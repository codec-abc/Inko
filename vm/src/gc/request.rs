//! Garbage Collection Requests
//!
//! A garbage collection request specifies what to collect (a heap or mailbox),
//! and what process to collect.

use gc::heap_collector;
use gc::mailbox_collector;
use gc::profile::Profile;
use process::RcProcess;
use vm::state::RcState;

pub enum CollectionType {
    Heap,
    Mailbox,
}

pub struct Request {
    pub vm_state: RcState,
    pub collection_type: CollectionType,
    pub process: RcProcess,
    pub profile: Profile,
}

impl Request {
    pub fn new(
        collection_type: CollectionType,
        vm_state: RcState,
        process: RcProcess,
    ) -> Self {
        let profile = match collection_type {
            CollectionType::Heap => {
                if process.should_collect_mature_generation() {
                    Profile::full()
                } else {
                    Profile::young()
                }
            }
            CollectionType::Mailbox => Profile::mailbox(),
        };

        Request {
            vm_state: vm_state,
            collection_type: collection_type,
            process: process,
            profile: profile,
        }
    }

    /// Returns a request for collecting a process' heap.
    pub fn heap(vm_state: RcState, process: RcProcess) -> Self {
        Self::new(CollectionType::Heap, vm_state, process)
    }

    /// Returns a request for collecting a process' mailbox.
    pub fn mailbox(vm_state: RcState, process: RcProcess) -> Self {
        Self::new(CollectionType::Mailbox, vm_state, process)
    }

    /// Performs the garbage collection request.
    pub fn perform(&mut self) {
        match self.collection_type {
            CollectionType::Heap => heap_collector::collect(
                &self.vm_state,
                &self.process,
                &mut self.profile,
            ),
            CollectionType::Mailbox => mailbox_collector::collect(
                &self.vm_state,
                &self.process,
                &mut self.profile,
            ),
        };

        println!(
            "Finished {:?} collection in {:.2} ms, {:.2} ms tracing, \
             {:.2} ms reclaiming, {:.2} ms finalizing, {:.2} ms suspended, \
             {} marked, {} promoted, {} evacuated",
            self.profile.collection_type,
            self.profile.total.duration_msec(),
            self.profile.trace.duration_msec(),
            self.profile.reclaim.duration_msec(),
            self.profile.finalize.duration_msec(),
            self.profile.suspended.duration_msec(),
            self.profile.marked,
            self.profile.promoted,
            self.profile.evacuated
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::Config;
    use vm::state::State;
    use vm::test::setup;

    #[test]
    fn test_new() {
        let (_machine, _block, process) = setup();
        let state = State::new(Config::new());
        let request = Request::new(CollectionType::Heap, state, process);

        assert!(match request.collection_type {
            CollectionType::Heap => true,
            CollectionType::Mailbox => false,
        });
    }

    #[test]
    fn test_heap() {
        let (_machine, _block, process) = setup();
        let state = State::new(Config::new());
        let request = Request::heap(state, process);

        assert!(match request.collection_type {
            CollectionType::Heap => true,
            CollectionType::Mailbox => false,
        });
    }

    #[test]
    fn test_mailbox() {
        let (_machine, _block, process) = setup();
        let state = State::new(Config::new());
        let request = Request::mailbox(state, process);

        assert!(match request.collection_type {
            CollectionType::Heap => false,
            CollectionType::Mailbox => true,
        });
    }

    #[test]
    fn test_perform() {
        let (_machine, _block, process) = setup();
        let state = State::new(Config::new());
        let mut request = Request::heap(state, process.clone());

        process.set_register(0, process.allocate_empty());
        process.running();
        request.perform();

        assert!(process.get_register(0).is_marked());
    }
}
