//! Process-local memory allocator
//!
//! The LocalAllocator lives in a Process and is used for allocating memory on a
//! process heap.

use immix::allocation_result::AllocationResult;
use immix::copy_object::CopyObject;
use immix::bucket::Bucket;
use immix::global_allocator::RcGlobalAllocator;

use object::Object;
use object_value;
use object_value::ObjectValue;
use object_pointer::ObjectPointer;

/// The maximum age of a bucket in the young generation.
pub const YOUNG_MAX_AGE: isize = 3;

/// Structure containing the state of a process-local allocator.
pub struct LocalAllocator {
    /// The global allocated from which to request blocks of memory and return
    /// unused blocks to.
    pub global_allocator: RcGlobalAllocator,

    /// The buckets to use for the eden and young survivor spaces.
    pub young_generation: [Bucket; 4],

    /// The position of the eden bucket in the young generation.
    pub eden_index: usize,

    /// The bucket to use for the mature generation.
    pub mature_generation: Bucket,
}

impl LocalAllocator {
    pub fn new(global_allocator: RcGlobalAllocator) -> LocalAllocator {
        // Prepare the eden bucket
        let mut eden = Bucket::with_age(0);
        let (block, _) = global_allocator.request_block();

        eden.add_block(block);

        LocalAllocator {
            global_allocator: global_allocator,
            young_generation: [eden,
                               Bucket::with_age(-1),
                               Bucket::with_age(-2),
                               Bucket::with_age(-3)],
            eden_index: 0,
            mature_generation: Bucket::new(),
        }
    }

    pub fn global_allocator(&self) -> RcGlobalAllocator {
        self.global_allocator.clone()
    }

    pub fn eden_space_mut(&mut self) -> &mut Bucket {
        &mut self.young_generation[self.eden_index]
    }

    pub fn mature_generation_mut(&mut self) -> &mut Bucket {
        &mut self.mature_generation
    }

    /// Resets and returns all blocks of all buckets to the global allocator.
    pub fn return_blocks(&mut self) {
        let mut blocks = Vec::new();

        for bucket in self.young_generation.iter_mut() {
            for block in bucket.blocks.drain(0..) {
                blocks.push(block);
            }
        }

        for block in self.mature_generation.blocks.drain(0..) {
            blocks.push(block);
        }

        for mut block in blocks {
            block.reset();
            self.global_allocator.add_block(block);
        }
    }

    /// Allocates an object with a prototype.
    pub fn allocate_with_prototype(&mut self,
                                   value: ObjectValue,
                                   proto: ObjectPointer)
                                   -> AllocationResult {
        let object = Object::with_prototype(value, proto);

        self.allocate_eden(object)
    }

    /// Allocates an object without a prototype.
    pub fn allocate_without_prototype(&mut self,
                                      value: ObjectValue)
                                      -> AllocationResult {
        let object = Object::new(value);

        self.allocate_eden(object)
    }

    /// Allocates an empty object without a prototype.
    pub fn allocate_empty(&mut self) -> AllocationResult {
        self.allocate_without_prototype(object_value::none())
    }

    /// Allocates an object in the eden space.
    pub fn allocate_eden(&mut self, object: Object) -> AllocationResult {
        // Try to allocate into the first available block.
        {
            if let Some(block) = self.eden_space_mut()
                .first_available_block() {
                return (block.bump_allocate(object), false);
            }
        }

        let (block, allocated_new) = self.global_allocator.request_block();
        let mut bucket = self.eden_space_mut();

        bucket.add_block(block);

        (bucket.bump_allocate(object), allocated_new)
    }

    /// Allocates an object in the mature space.
    pub fn allocate_mature(&mut self, object: Object) -> AllocationResult {
        // Try to allocate into the first available block.
        {
            if let Some(block) = self.mature_generation_mut()
                .first_available_block() {
                return (block.bump_allocate(object), false);
            }
        }

        let (block, allocated_new) = self.global_allocator.request_block();
        let mut bucket = self.mature_generation_mut();

        bucket.add_block(block);

        (bucket.bump_allocate(object), allocated_new)
    }

    /// Increments the age of all buckets in the young generation
    pub fn increment_young_ages(&mut self) {
        for (index, bucket) in self.young_generation.iter_mut().enumerate() {
            if bucket.age == YOUNG_MAX_AGE {
                bucket.reset_age();
                self.eden_index = index;
            } else {
                bucket.increment_age();
            }
        }
    }
}

impl CopyObject for LocalAllocator {
    fn allocate_copy(&mut self, object: Object) -> AllocationResult {
        self.allocate_eden(object)
    }
}