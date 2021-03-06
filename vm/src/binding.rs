//! Variable Bindings
//!
//! A binding contains the local variables available to a certain scope.
use std::cell::UnsafeCell;

use arc_without_weak::ArcWithoutWeak;
use block::Block;
use chunk::Chunk;
use gc::work_list::WorkList;
use immix::copy_object::CopyObject;
use object_pointer::{ObjectPointer, ObjectPointerPointer};

pub struct Binding {
    /// The local variables in the current binding.
    ///
    /// Local variables must **not** be modified concurrently as access is not
    /// synchronized due to 99% of all operations being process-local.
    pub locals: UnsafeCell<Chunk<ObjectPointer>>,

    /// The parent binding, if any.
    pub parent: Option<RcBinding>,
}

pub struct PointerIterator<'a> {
    binding: &'a Binding,
    local_index: usize,
}

pub type RcBinding = ArcWithoutWeak<Binding>;

impl Binding {
    /// Returns a new binding.
    pub fn new(amount: usize) -> RcBinding {
        let bind = Binding {
            locals: UnsafeCell::new(Chunk::new(amount)),
            parent: None,
        };

        ArcWithoutWeak::new(bind)
    }

    /// Returns a new binding with a parent binding and a defalt capacity for
    /// the local variables.
    pub fn with_parent(parent_binding: RcBinding, amount: usize) -> RcBinding {
        let bind = Binding {
            locals: UnsafeCell::new(Chunk::new(amount)),
            parent: Some(parent_binding),
        };

        ArcWithoutWeak::new(bind)
    }

    #[inline(always)]
    pub fn from_block(block: &Block) -> RcBinding {
        if block.code.captures {
            Binding::with_parent(block.binding.clone(), block.locals())
        } else {
            Binding::new(block.locals())
        }
    }

    /// Returns the value of a local variable.
    pub fn get_local(&self, index: usize) -> ObjectPointer {
        self.locals()[index]
    }

    /// Sets a local variable.
    pub fn set_local(&self, index: usize, value: ObjectPointer) {
        self.locals_mut()[index] = value;
    }

    /// Returns true if the local variable exists.
    pub fn local_exists(&self, index: usize) -> bool {
        !self.get_local(index).is_null()
    }

    /// Returns the parent binding.
    pub fn parent(&self) -> Option<RcBinding> {
        self.parent.clone()
    }

    /// Tries to find a parent binding while limiting the amount of bindings to
    /// traverse.
    pub fn find_parent(&self, depth: usize) -> Option<RcBinding> {
        let mut found = self.parent();

        for _ in 0..depth {
            if let Some(unwrapped) = found {
                found = unwrapped.parent();
            } else {
                return None;
            }
        }

        found
    }

    /// Returns an immutable reference to this binding's local variables.
    pub fn locals(&self) -> &Chunk<ObjectPointer> {
        unsafe { &*self.locals.get() }
    }

    /// Returns a mutable reference to this binding's local variables.
    #[cfg_attr(feature = "cargo-clippy", allow(mut_from_ref))]
    pub fn locals_mut(&self) -> &mut Chunk<ObjectPointer> {
        unsafe { &mut *self.locals.get() }
    }

    /// Pushes all pointers in this binding into the supplied vector.
    pub fn push_pointers(&self, pointers: &mut WorkList) {
        for pointer in self.pointers() {
            pointers.push(pointer);
        }
    }

    /// Returns an iterator for traversing all pointers in this binding.
    pub fn pointers(&self) -> PointerIterator {
        PointerIterator {
            binding: self,
            local_index: 0,
        }
    }

    /// Creates a new binding and recursively copies over all pointers to the
    /// target heap.
    pub fn clone_to<H: CopyObject>(&self, heap: &mut H) -> RcBinding {
        let parent = if let Some(ref bind) = self.parent {
            Some(bind.clone_to(heap))
        } else {
            None
        };

        let locals = self.locals();
        let mut new_locals = Chunk::new(locals.len());

        for index in 0..locals.len() {
            let pointer = locals[index];

            if !pointer.is_null() {
                new_locals[index] = heap.copy_object(pointer);
            }
        }

        ArcWithoutWeak::new(Binding {
            locals: UnsafeCell::new(new_locals),
            parent,
        })
    }

    // Moves all pointers in this binding to the given heap.
    #[cfg_attr(feature = "cargo-clippy", allow(needless_range_loop))]
    pub fn move_pointers_to<H: CopyObject>(&self, heap: &mut H) {
        if let Some(ref bind) = self.parent {
            bind.move_pointers_to(heap);
        }

        let locals = self.locals_mut();

        for index in 0..locals.len() {
            let pointer = locals[index];

            if !pointer.is_null() {
                locals[index] = heap.move_object(pointer);
            }
        }
    }
}

impl<'a> Iterator for PointerIterator<'a> {
    type Item = ObjectPointerPointer;

    fn next(&mut self) -> Option<ObjectPointerPointer> {
        loop {
            while self.local_index < self.binding.locals().len() {
                let local = &self.binding.locals()[self.local_index];

                self.local_index += 1;

                if local.is_null() {
                    continue;
                }

                return Some(local.pointer());
            }

            if self.binding.parent.is_some() {
                self.binding = self.binding.parent.as_ref().unwrap();
                self.local_index = 0;
            } else {
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::Config;
    use immix::global_allocator::GlobalAllocator;
    use immix::local_allocator::LocalAllocator;
    use object_pointer::ObjectPointer;
    use object_value;

    #[test]
    fn test_new() {
        let binding = Binding::new(2);

        assert_eq!(binding.locals().len(), 2);
    }

    #[test]
    fn test_with_parent() {
        let binding1 = Binding::new(0);
        let binding2 = Binding::with_parent(binding1.clone(), 1);

        assert!(binding2.parent.is_some());
        assert_eq!(binding2.locals().len(), 1);
    }

    #[test]
    fn test_get_local_valid() {
        let ptr = ObjectPointer::integer(5);
        let binding = Binding::new(1);

        binding.set_local(0, ptr);

        assert!(binding.get_local(0) == ptr);
    }

    #[test]
    fn test_set_local() {
        let ptr = ObjectPointer::integer(5);
        let binding = Binding::new(1);

        binding.set_local(0, ptr);

        assert_eq!(binding.locals().len(), 1);
    }

    #[test]
    fn test_local_exists_non_existing_local() {
        let binding = Binding::new(1);

        assert_eq!(binding.local_exists(0), false);
    }

    #[test]
    fn test_local_exists_existing_local() {
        let ptr = ObjectPointer::integer(5);
        let binding = Binding::new(1);

        binding.set_local(0, ptr);

        assert!(binding.local_exists(0));
    }

    #[test]
    fn test_parent_without_parent() {
        let binding = Binding::new(0);

        assert!(binding.parent().is_none());
    }

    #[test]
    fn test_parent_with_parent() {
        let binding1 = Binding::new(0);
        let binding2 = Binding::with_parent(binding1, 0);

        assert!(binding2.parent().is_some());
    }

    #[test]
    fn test_find_parent_without_parent() {
        let binding = Binding::new(0);

        assert!(binding.find_parent(0).is_none());
    }

    #[test]
    fn test_find_parent_with_parent() {
        let binding1 = Binding::new(0);
        let binding2 = Binding::with_parent(binding1, 0);
        let binding3 = Binding::with_parent(binding2, 0);
        let binding4 = Binding::with_parent(binding3, 0);

        assert!(binding4.find_parent(0).is_some());
        assert!(binding4.find_parent(0).unwrap().parent().is_some());

        assert!(binding4.find_parent(1).is_some());
        assert!(binding4.find_parent(1).unwrap().parent().is_some());

        assert!(binding4.find_parent(2).is_some());
        assert!(binding4.find_parent(2).unwrap().parent().is_none());

        assert!(binding4.find_parent(3).is_none());
    }

    #[test]
    fn test_locals() {
        let ptr = ObjectPointer::integer(5);
        let binding = Binding::new(1);

        binding.set_local(0, ptr);

        assert_eq!(binding.locals().len(), 1);
    }

    #[test]
    fn test_locals_mut() {
        let ptr = ObjectPointer::integer(5);
        let binding = Binding::new(1);

        binding.set_local(0, ptr);

        assert_eq!(binding.locals_mut().len(), 1);
    }

    #[test]
    fn test_push_pointers() {
        let mut alloc =
            LocalAllocator::new(GlobalAllocator::new(), &Config::new());

        let local1 = alloc.allocate_empty();
        let binding1 = Binding::new(1);

        binding1.set_local(0, local1);

        let local2 = alloc.allocate_empty();
        let binding2 = Binding::with_parent(binding1.clone(), 1);

        binding2.set_local(0, local2);

        let mut pointers = WorkList::new();

        binding2.push_pointers(&mut pointers);

        assert!(*pointers.pop().unwrap().get() == local2);
        assert!(*pointers.pop().unwrap().get() == local1);
    }

    #[test]
    fn test_pointers() {
        let mut alloc =
            LocalAllocator::new(GlobalAllocator::new(), &Config::new());

        let b1_local1 = alloc.allocate_empty();
        let b1_local2 = alloc.allocate_empty();
        let b1 = Binding::new(2);

        b1.set_local(0, b1_local1);
        b1.set_local(1, b1_local2);

        let b2_local1 = alloc.allocate_empty();
        let b2_local2 = alloc.allocate_empty();
        let b2 = Binding::with_parent(b1.clone(), 2);

        b2.set_local(0, b2_local1);
        b2.set_local(1, b2_local2);

        let mut iterator = b2.pointers();

        assert!(iterator.next().unwrap().get() == &b2_local1);
        assert!(iterator.next().unwrap().get() == &b2_local2);

        assert!(iterator.next().unwrap().get() == &b1_local1);
        assert!(iterator.next().unwrap().get() == &b1_local2);

        assert!(iterator.next().is_none());
    }

    #[test]
    fn test_clone_to() {
        let global_alloc = GlobalAllocator::new();
        let mut alloc1 =
            LocalAllocator::new(global_alloc.clone(), &Config::new());
        let mut alloc2 = LocalAllocator::new(global_alloc, &Config::new());

        let ptr1 = alloc1.allocate_without_prototype(object_value::float(5.0));
        let ptr2 = alloc1.allocate_without_prototype(object_value::float(2.0));

        let src_bind1 = Binding::new(1);
        let src_bind2 = Binding::with_parent(src_bind1.clone(), 1);

        src_bind1.set_local(0, ptr1);
        src_bind2.set_local(0, ptr2);

        let bind_copy = src_bind2.clone_to(&mut alloc2);

        assert_eq!(bind_copy.locals().len(), 1);
        assert!(bind_copy.parent.is_some());

        assert_eq!(bind_copy.get_local(0).float_value().unwrap(), 2.0);

        let bind_copy_parent = bind_copy.parent.as_ref().unwrap();

        assert_eq!(bind_copy_parent.locals().len(), 1);
        assert!(bind_copy_parent.parent.is_none());

        assert_eq!(bind_copy_parent.get_local(0).float_value().unwrap(), 5.0);
    }

    #[test]
    fn test_move_pointers_to() {
        let galloc = GlobalAllocator::new();
        let mut alloc1 = LocalAllocator::new(galloc.clone(), &Config::new());
        let mut alloc2 = LocalAllocator::new(galloc, &Config::new());

        let ptr1 = alloc1.allocate_without_prototype(object_value::float(5.0));
        let ptr2 = alloc1.allocate_without_prototype(object_value::float(2.0));

        let src_bind1 = Binding::new(1);
        let src_bind2 = Binding::with_parent(src_bind1.clone(), 1);

        src_bind1.set_local(0, ptr1);
        src_bind2.set_local(0, ptr2);
        src_bind2.move_pointers_to(&mut alloc2);

        // The original pointers now point to empty objects.
        assert!(ptr1.get().value.is_none());
        assert!(ptr2.get().value.is_none());

        assert_eq!(src_bind2.get_local(0).float_value().unwrap(), 2.0);
        assert_eq!(src_bind1.get_local(0).float_value().unwrap(), 5.0);
    }
}
