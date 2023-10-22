use std::cell::RefCell;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use fnv::FnvHashMap as HashMap;
use mrpc_marshal::SgList;
use nix::unistd::Pid;
use phoenix_api::Handle;
use phoenix_salloc::region::AddressMediator;

use phoenix_common::state_mgr::ProcessShared;

use super::pool::{BufferPool, RecvBuffer};

pub(crate) struct State {
    // shared among all engines of a user process
    pub(crate) shared: Arc<Shared>,
}
// SAFETY: State in tcp will not be shared by multiple threads
// It is owned and used by a single thread/runtime
unsafe impl Sync for State {}

impl State {
    pub fn new(shared: Arc<Shared>) -> Self {
        State { shared }
    }
}

impl Clone for State {
    fn clone(&self) -> Self {
        State {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl State {
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn resource(&self) -> &Resource {
        &self.shared.resource
    }

    #[inline]
    #[allow(dead_code)]
    pub(crate) fn acceptor_should_stop(&self) -> bool {
        self.shared.stop_acceptor.load(Ordering::Relaxed)
    }

    #[inline]
    #[allow(dead_code)]
    pub(crate) fn stop_acceptor(&self, stop: bool) {
        self.shared.stop_acceptor.store(stop, Ordering::Relaxed);
    }
}

pub struct Shared {
    pub(crate) pid: Pid,
    alive_engines: AtomicUsize,
    stop_acceptor: AtomicBool,
    resource: Resource,
}

impl ProcessShared for Shared {
    type Err = io::Error;

    fn new(_pid: Pid) -> io::Result<Self> {
        panic!("should not use this function")
    }
}

impl Shared {
    pub(crate) fn new_from_addr_mediator(
        pid: Pid,
        addr_mediator: Arc<AddressMediator>,
    ) -> io::Result<Self> {
        let shared = Shared {
            pid,
            stop_acceptor: AtomicBool::new(false),
            alive_engines: AtomicUsize::new(1),
            resource: Resource::new(addr_mediator),
        };
        Ok(shared)
    }
}

pub(crate) struct Resource {
    pub(crate) recv_buffer_pool: BufferPool,
}

impl Resource {
    pub(crate) fn new(addr_mediator: Arc<AddressMediator>) -> Self {
        Resource {
            recv_buffer_pool: BufferPool::new(addr_mediator),
        }
    }
}
