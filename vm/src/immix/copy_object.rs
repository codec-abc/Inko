//! Copying Objects
//!
//! The CopyObject trait can be implemented by allocators to support copying of
//! objects into a heap.

use block::Block;
use object::{AttributesMap, Object};
use object_pointer::ObjectPointer;
use object_value;
use object_value::ObjectValue;

pub trait CopyObject: Sized {
    /// Allocates a copied object.
    fn allocate_copy(&mut self, Object) -> ObjectPointer;

    /// Performs a deep copy of the given pointer.
    ///
    /// The copy of the input object is allocated on the current heap.
    fn copy_object(&mut self, to_copy_ptr: ObjectPointer) -> ObjectPointer {
        if to_copy_ptr.is_permanent() {
            return to_copy_ptr;
        }

        let to_copy = to_copy_ptr.get();

        // Copy over the object value
        let value_copy = match to_copy.value {
            ObjectValue::None => object_value::none(),
            ObjectValue::Float(num) => object_value::float(num),
            ObjectValue::Integer(num) => object_value::integer(num),
            ObjectValue::BigInt(ref bigint) => {
                ObjectValue::BigInt(bigint.clone())
            }
            ObjectValue::String(ref string) => {
                ObjectValue::String(string.clone())
            }
            ObjectValue::InternedString(ref string) => {
                object_value::interned_string(*string.clone())
            }
            ObjectValue::Array(ref raw_vec) => {
                let new_map =
                    raw_vec.iter().map(|val_ptr| self.copy_object(*val_ptr));

                object_value::array(new_map.collect::<Vec<_>>())
            }
            ObjectValue::File(_) => {
                panic!("ObjectValue::File can not be cloned");
            }
            ObjectValue::Block(ref block) => {
                let new_binding = block.binding.clone_to(self);
                let new_scope = block.global_scope;
                let new_block = Block::new(block.code, new_binding, new_scope);

                object_value::block(new_block)
            }
            ObjectValue::Binding(ref binding) => {
                let new_binding = binding.clone_to(self);

                object_value::binding(new_binding)
            }
            ObjectValue::Hasher(ref hasher) => {
                ObjectValue::Hasher((*hasher).clone())
            }
            ObjectValue::ByteArray(ref byte_array) => {
                ObjectValue::ByteArray(byte_array.clone())
            }
        };

        let mut copy = if let Some(proto_ptr) = to_copy.prototype() {
            let proto_copy = self.copy_object(proto_ptr);

            Object::with_prototype(value_copy, proto_copy)
        } else {
            Object::new(value_copy)
        };

        if let Some(map) = to_copy.attributes_map() {
            let mut map_copy = AttributesMap::default();

            for (key, val) in map.iter() {
                let key_copy = self.copy_object(*key);
                let val_copy = self.copy_object(*val);

                map_copy.insert(key_copy, val_copy);
            }

            copy.set_attributes_map(map_copy);
        }

        self.allocate_copy(copy)
    }

    /// Performs a deep move of the given pointer.
    ///
    /// This will copy over the object to the current heap, while _moving_ all
    /// related data from the old object into the new one.
    #[cfg_attr(feature = "cargo-clippy", allow(needless_range_loop))]
    fn move_object(&mut self, to_copy_ptr: ObjectPointer) -> ObjectPointer {
        if to_copy_ptr.is_permanent() {
            return to_copy_ptr;
        }

        let to_copy = to_copy_ptr.get_mut();

        let value_copy = match to_copy.value.take() {
            ObjectValue::Array(mut array) => {
                for index in 0..array.len() {
                    array[index] = self.move_object(array[index]);
                }

                ObjectValue::Array(array)
            }
            ObjectValue::Block(block) => {
                block.binding.move_pointers_to(self);

                ObjectValue::Block(block)
            }
            ObjectValue::Binding(binding) => {
                binding.move_pointers_to(self);

                ObjectValue::Binding(binding)
            }
            value => value,
        };

        let mut copy = if let Some(proto_ptr) = to_copy.take_prototype() {
            let proto_copy = self.move_object(proto_ptr);

            Object::with_prototype(value_copy, proto_copy)
        } else {
            Object::new(value_copy)
        };

        if let Some(map) = to_copy.attributes_map() {
            let mut map_copy = AttributesMap::default();

            for (key, val) in map.iter() {
                let key_copy = self.move_object(*key);
                let val_copy = self.move_object(*val);

                map_copy.insert(key_copy, val_copy);
            }

            copy.set_attributes_map(map_copy);
        }

        to_copy.drop_attributes();
        to_copy_ptr.unmark_for_finalization();

        self.allocate_copy(copy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use binding::Binding;
    use compiled_code::CompiledCode;
    use config::Config;
    use deref_pointer::DerefPointer;
    use global_scope::{GlobalScope, GlobalScopePointer};
    use immix::global_allocator::GlobalAllocator;
    use immix::local_allocator::LocalAllocator;
    use object::Object;
    use object_pointer::ObjectPointer;
    use object_value;
    use vm::state::{RcState, State};

    struct DummyAllocator {
        pub allocator: LocalAllocator,
    }

    impl DummyAllocator {
        pub fn new() -> DummyAllocator {
            let global_alloc = GlobalAllocator::new();

            DummyAllocator {
                allocator: LocalAllocator::new(global_alloc, &Config::new()),
            }
        }
    }

    impl CopyObject for DummyAllocator {
        fn allocate_copy(&mut self, object: Object) -> ObjectPointer {
            self.allocator.allocate_copy(object)
        }
    }

    fn state() -> RcState {
        State::new(Config::new())
    }

    #[test]
    fn test_copy_none() {
        let mut dummy = DummyAllocator::new();
        let pointer = dummy.allocator.allocate_empty();
        let copy = dummy.copy_object(pointer);

        assert!(copy.get().value.is_none());
    }

    #[test]
    fn test_copy_with_prototype() {
        let mut dummy = DummyAllocator::new();
        let pointer = dummy.allocator.allocate_empty();
        let proto = dummy.allocator.allocate_empty();

        pointer.get_mut().set_prototype(proto);

        let copy = dummy.copy_object(pointer);

        assert!(copy.get().prototype().is_some());
    }

    #[test]
    fn test_copy_with_attributes() {
        let mut dummy = DummyAllocator::new();
        let ptr1 = dummy.allocator.allocate_empty();
        let ptr2 = dummy.allocator.allocate_empty();
        let name = dummy.allocator.allocate_empty();

        ptr1.get_mut().add_attribute(name, ptr2);
        ptr1.mark_for_finalization();

        let copy = dummy.copy_object(ptr1);

        assert!(copy.get().attributes_map().is_some());
    }

    #[test]
    fn test_copy_integer() {
        let mut dummy = DummyAllocator::new();
        let pointer = dummy
            .allocator
            .allocate_without_prototype(object_value::integer(5));

        let copy = dummy.copy_object(pointer);

        assert!(copy.get().value.is_integer());
        assert_eq!(copy.integer_value().unwrap(), 5);
    }

    #[test]
    fn test_copy_float() {
        let mut dummy = DummyAllocator::new();
        let pointer = dummy
            .allocator
            .allocate_without_prototype(object_value::float(2.5));

        let copy = dummy.copy_object(pointer);

        assert!(copy.get().value.is_float());
        assert_eq!(copy.get().value.as_float().unwrap(), 2.5);
    }

    #[test]
    fn test_copy_string() {
        let mut dummy = DummyAllocator::new();
        let pointer = dummy
            .allocator
            .allocate_without_prototype(object_value::string("a".to_string()));

        let copy = dummy.copy_object(pointer);

        assert!(copy.get().value.is_string());
        assert_eq!(copy.string_value().unwrap(), &"a".to_string());
    }

    #[test]
    fn test_copy_array() {
        let mut dummy = DummyAllocator::new();
        let ptr1 = dummy.allocator.allocate_empty();
        let ptr2 = dummy.allocator.allocate_empty();
        let array = dummy
            .allocator
            .allocate_without_prototype(object_value::array(vec![ptr1, ptr2]));

        let copy = dummy.copy_object(array);

        assert!(copy.get().value.is_array());
        assert_eq!(copy.get().value.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_copy_block() {
        let mut dummy = DummyAllocator::new();
        let state = state();
        let cc = CompiledCode::new(
            state.intern_owned("a".to_string()),
            state.intern_owned("a".to_string()),
            1,
            Vec::new(),
        );

        let scope = GlobalScope::new();

        let block = Block::new(
            DerefPointer::new(&cc),
            Binding::new(0),
            GlobalScopePointer::new(&scope),
        );

        let ptr = dummy
            .allocator
            .allocate_without_prototype(object_value::block(block));

        let copy = dummy.copy_object(ptr);

        assert!(copy.get().value.is_block());
    }

    #[test]
    fn test_copy_binding() {
        let mut dummy = DummyAllocator::new();

        let binding1 = Binding::new(1);
        let binding2 = Binding::with_parent(binding1.clone(), 1);

        let local1 = dummy
            .allocator
            .allocate_without_prototype(object_value::float(15.0));

        let local2 = dummy
            .allocator
            .allocate_without_prototype(object_value::float(20.0));

        binding1.set_local(0, local1);
        binding2.set_local(0, local2);

        let binding_ptr = dummy
            .allocator
            .allocate_without_prototype(object_value::binding(binding2));

        let binding_copy_ptr = dummy.copy_object(binding_ptr);
        let binding_copy_obj = binding_copy_ptr.get();

        let binding_copy = binding_copy_obj.value.as_binding().unwrap();
        let parent_copy = binding_copy.parent.clone().unwrap();

        assert!(binding_copy.parent.is_some());

        let local1_copy = binding_copy.get_local(0);
        let local2_copy = parent_copy.get_local(0);

        assert!(local1 != local1_copy);
        assert!(local2 != local2_copy);

        assert_eq!(local1_copy.float_value().unwrap(), 20.0);
        assert_eq!(local2_copy.float_value().unwrap(), 15.0);
    }

    #[test]
    fn test_move_none() {
        let mut dummy = DummyAllocator::new();
        let pointer = dummy.allocator.allocate_empty();
        let copy = dummy.move_object(pointer);

        assert!(copy.get().value.is_none());
    }

    #[test]
    fn test_move_with_prototype() {
        let mut dummy = DummyAllocator::new();
        let pointer = dummy.allocator.allocate_empty();
        let proto = dummy.allocator.allocate_empty();

        pointer.get_mut().set_prototype(proto);

        let copy = dummy.move_object(pointer);

        assert!(copy.get().prototype().is_some());
        assert!(pointer.get().prototype().is_none());
    }

    #[test]
    fn test_move_with_attributes() {
        let mut dummy = DummyAllocator::new();
        let ptr1 = dummy.allocator.allocate_empty();
        let ptr2 = dummy.allocator.allocate_empty();
        let name = dummy.allocator.allocate_empty();

        ptr1.get_mut().add_attribute(name, ptr2);
        ptr1.mark_for_finalization();

        let copy = dummy.move_object(ptr1);

        assert_eq!(ptr1.is_finalizable(), false);
        assert!(ptr1.get().attributes_map().is_none());

        assert_eq!(copy.is_finalizable(), true);
        assert!(copy.get().attributes_map().is_some());
    }

    #[test]
    fn test_move_integer() {
        let mut dummy = DummyAllocator::new();
        let pointer = dummy
            .allocator
            .allocate_without_prototype(object_value::integer(5));

        let copy = dummy.move_object(pointer);

        assert!(pointer.get().value.is_none());

        assert!(copy.get().value.is_integer());
        assert_eq!(copy.integer_value().unwrap(), 5);
    }

    #[test]
    fn test_move_float() {
        let mut dummy = DummyAllocator::new();
        let pointer = dummy
            .allocator
            .allocate_without_prototype(object_value::float(2.5));

        let copy = dummy.move_object(pointer);

        assert!(pointer.get().value.is_none());

        assert!(copy.get().value.is_float());
        assert_eq!(copy.get().value.as_float().unwrap(), 2.5);
    }

    #[test]
    fn test_move_string() {
        let mut dummy = DummyAllocator::new();
        let pointer = dummy
            .allocator
            .allocate_without_prototype(object_value::string("a".to_string()));

        let copy = dummy.move_object(pointer);

        assert!(pointer.get().value.is_none());

        assert!(copy.get().value.is_string());
        assert_eq!(copy.string_value().unwrap(), &"a".to_string());
    }

    #[test]
    fn test_move_array() {
        let mut dummy = DummyAllocator::new();
        let ptr1 = dummy.allocator.allocate_empty();
        let ptr2 = dummy.allocator.allocate_empty();
        let array = dummy
            .allocator
            .allocate_without_prototype(object_value::array(vec![ptr1, ptr2]));

        let copy = dummy.move_object(array);

        assert!(array.get().value.is_none());

        assert!(copy.get().value.is_array());
        assert_eq!(copy.get().value.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_move_block() {
        let mut dummy = DummyAllocator::new();
        let state = state();
        let cc = CompiledCode::new(
            state.intern_owned("a".to_string()),
            state.intern_owned("a".to_string()),
            1,
            Vec::new(),
        );

        let scope = GlobalScope::new();

        let block = Block::new(
            DerefPointer::new(&cc),
            Binding::new(0),
            GlobalScopePointer::new(&scope),
        );

        let ptr = dummy
            .allocator
            .allocate_without_prototype(object_value::block(block));

        let copy = dummy.move_object(ptr);

        assert!(ptr.get().value.is_none());
        assert!(copy.get().value.is_block());
    }

    #[test]
    fn test_move_binding() {
        let mut dummy = DummyAllocator::new();

        let binding1 = Binding::new(1);
        let binding2 = Binding::with_parent(binding1.clone(), 1);

        let local1 = dummy
            .allocator
            .allocate_without_prototype(object_value::float(15.0));

        let local2 = dummy
            .allocator
            .allocate_without_prototype(object_value::float(20.0));

        binding1.set_local(0, local1);
        binding2.set_local(0, local2);

        let binding_ptr = dummy
            .allocator
            .allocate_without_prototype(object_value::binding(binding2));

        let binding_move_ptr = dummy.move_object(binding_ptr);
        let binding_move = binding_move_ptr.get();

        assert!(binding_move.value.as_binding().unwrap().local_exists(0));
        assert!(binding_ptr.get().value.is_none());
    }
}
