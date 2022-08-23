use std::pin::Pin;

use futures::future::BoxFuture;

use super::super::state::State;
use super::super::ApiError;
use crate::engine::{future, Engine, EngineResult, Indicator};
use crate::node::Node;

pub struct CmEngine {
    pub(crate) node: Node,
    pub(crate) indicator: Indicator,
    pub(crate) state: State,
}

impl CmEngine {
    pub(crate) fn new(node: Node, state: State) -> Self {
        Self {
            node,
            indicator: Default::default(),
            state,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
}

use Status::Progress;

crate::unimplemented_ungradable!(CmEngine);
crate::impl_vertex_for_engine!(CmEngine, node);

impl Engine for CmEngine {
    fn description(self: Pin<&Self>) -> String {
        format!("RDMA CmEngine, user: {:?}", self.get_ref().state.shared.pid)
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }
}

impl CmEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut nwork = 0;
            let Progress(n) = self.check_cm_event()?;
            nwork += n;
            if self.state.alive_engines() == 1 {
                // cm_engine is the last active engine
                return Ok(());
            }

            self.indicator.set_nwork(nwork);
            future::yield_now().await;
        }
    }
}

impl CmEngine {
    fn check_cm_event(&mut self) -> Result<Status, ApiError> {
        match self.poll_cm_event_once() {
            Ok(()) => Ok(Progress(1)),
            Err(ApiError::NoCmEvent) => Ok(Progress(0)),
            Err(e @ (ApiError::RdmaCm(_) | ApiError::Mio(_))) => {
                self.state
                    .shared
                    .cm_manager
                    .blocking_lock()
                    .err_buffer
                    .push_back(e);
                return Ok(Progress(1));
            }
            Err(e) => return Err(e),
        }
    }

    fn poll_cm_event_once(&mut self) -> Result<(), ApiError> {
        // NOTE(cjr): It is fine to blocking_lock here, because we guarantee the CmEngine are
        // running on a separate Runtime.
        // TODO(cjr): the above assumption may not held anymore, fix this.
        self.state
            .shared
            .cm_manager
            .blocking_lock()
            .poll_cm_event_once(&self.state.resource().event_channel_table)
    }
}
