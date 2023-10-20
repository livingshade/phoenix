//! This engine can only be placed at the sender side for now.
use std::collections::BTreeMap;
use std::os::unix::ucred::UCred;
use std::pin::Pin;
use std::ptr::Unique;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use futures::SinkExt;

use phoenix_api::rpc::{RpcId, StatusCode};
use phoenix_api_policy_cache::control_plane;

use super::DatapathError;
use crate::config::CacheConfig;
use phoenix_common::engine::datapath::message::{
    EngineRxMessage, EngineTxMessage, RpcMessageRx, RpcMessageTx,
};
use phoenix_common::engine::datapath::meta_pool::MetaBufferPool;
use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::module::Version;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

pub mod cache {
    // The string specified here must match the proto package name
    include!("cache.rs");
}

pub(crate) struct CacheEngine {
    pub(crate) node: DataPathNode,

    pub(crate) indicator: Indicator,

    pub(crate) config: CacheConfig,

    pub(crate) meta_pool: MetaBufferPool,
    pub(crate) buffer: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for CacheEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "CacheEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig => {
                // Update config
                self.config = CacheConfig {};
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(CacheEngine, node);

impl Decompose for CacheEngine {
    fn flush(&mut self) -> Result<usize> {
        let mut work = 0;
        while !self.tx_inputs()[0].is_empty() || !self.rx_inputs()[0].is_empty() {
            if let Progress(n) = self.receiver_check_input_queue()? {
                work += n;
            }
        }
        Ok(work)
    }

    fn decompose(
        self: Box<Self>,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode) {
        let engine = *self;

        let mut collections = ResourceCollection::with_capacity(2);

        collections.insert("config".to_string(), Box::new(engine.config));
        collections.insert("buffer".to_string(), Box::new(engine.buffer));

        (collections, engine.node)
    }
}

impl CacheEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<CacheConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let buffer = *local
            .remove("buffer")
            .unwrap()
            .downcast::<BTreeMap<String, String>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let meta_pool: MetaBufferPool = *local
            .remove("meta_pool")
            .unwrap()
            .downcast::<MetaBufferPool>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = CacheEngine {
            node,
            indicator: Default::default(),
            meta_pool,
            buffer,
            config,
        };
        Ok(engine)
    }
}

impl CacheEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut work = 0;
            // check input queue, ~100ns
            loop {
                match self.receiver_check_input_queue()? {
                    Progress(0) => break,
                    Progress(n) => work += n,
                    Status::Disconnected => return Ok(()),
                }
            }

            // If there's pending receives, there will always be future work to do.
            self.indicator.set_nwork(work);

            future::yield_now().await;
        }
    }
}

/// Copy the RPC request to a private heap and returns the request.
#[inline]
fn get_key(msg: &RpcMessageTx) -> String {
    let req_ptr = Unique::new(msg.addr_backend as *mut cache::Key).unwrap();
    let req = unsafe { req_ptr.as_ref() };
    // returns a private_req
    let key = String::from_utf8_lossy(&req.key);
    key.to_string().clone()
}

#[inline]
fn get_value(msg: &RpcMessageRx) -> String {
    let req_ptr = Unique::new(msg.addr_backend as *mut cache::Value).unwrap();
    let req = unsafe { req_ptr.as_ref() };
    // returns a private_req
    let value = String::from_utf8_lossy(&req.value);
    value.to_string().clone()
}

impl CacheEngine {
    fn receiver_check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        let key = get_key(&msg);
                        if let Some(value) = self.buffer.get(&key) {
                            let ori_meta = msg.meta_buf_ptr.as_meta_ptr();
                            let meta = unsafe { (*ori_meta).clone() };
                            let resp = RpcMessageRx {
                                meta: Unique::new(&meta).unwrap(),
                                addr_app: msg.addr_backend,
                                addr_backend: msg.addr_backend,
                            };

                            self.rx_outputs()[0].send(EngineRxMessage::RpcMessage(resp))?;
                        } else {
                            self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
                        }
                    }
                    EngineTxMessage::ReclaimRecvBuf(_handle, _call_ids) => {
                        self.tx_outputs()[0].send(msg)?;
                    }
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        // forward all rx msgs
        match self.rx_inputs()[0].try_recv() {
            Ok(m) => {
                self.rx_outputs()[0].send(m)?;
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        Ok(Progress(0))
    }
}
