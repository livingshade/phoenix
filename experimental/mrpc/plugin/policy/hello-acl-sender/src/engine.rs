//! This engine can only be placed at the sender side for now.
use std::f32::consts::E;
use std::num::NonZeroU32;
use std::os::unix::ucred::UCred;
use std::pin::Pin;
use std::ptr::Unique;

use anyhow::{anyhow, Result};
use fnv::FnvHashMap as HashMap;
use futures::future::BoxFuture;

use phoenix_api::rpc::{RpcId, TransportStatus};
use phoenix_api_policy_hello_acl_sender::control_plane;

use phoenix_common::engine::datapath::message::{EngineRxMessage, EngineTxMessage, RpcMessageTx};
use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::log;
use phoenix_common::module::Version;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::HelloAclSenderConfig;

pub mod hello {
    // The string specified here must match the proto package name
    include!("rpc_hello.rs");
}

pub struct AclTable {
    pub name: String,
    pub permission: String,
}

pub(crate) struct HelloAclSenderEngine {
    pub(crate) node: DataPathNode,

    pub(crate) indicator: Indicator,

    pub(crate) outstanding_req_pool: HashMap<RpcId, Box<hello::HelloRequest>>,

    // A set of func_ids to apply the rate limit.
    // TODO(cjr): maybe put this filter in a separate engine like FilterEngine/ClassiferEngine.
    // pub(crate) filter: FnvHashSet<u32>,
    // Number of tokens to add for each seconds.
    pub(crate) config: HelloAclSenderConfig,
    pub(crate) acl_table: Vec<AclTable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for HelloAclSenderEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "HelloAclSenderEngine".to_owned()
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
                self.config = HelloAclSenderConfig {};
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(HelloAclSenderEngine, node);

impl Decompose for HelloAclSenderEngine {
    fn flush(&mut self) -> Result<usize> {
        let mut work = 0;
        while !self.tx_inputs()[0].is_empty() || !self.rx_inputs()[0].is_empty() {
            if let Progress(n) = self.check_input_queue()? {
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
        collections.insert(
            "outstanding_req_pool".to_string(),
            Box::new(engine.outstanding_req_pool),
        );
        collections.insert("config".to_string(), Box::new(engine.config));
        (collections, engine.node)
    }
}

impl HelloAclSenderEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let outstanding_req_pool = *local
            .remove("outstanding_req_pool")
            .unwrap()
            .downcast::<HashMap<RpcId, Box<hello::HelloRequest>>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<HelloAclSenderConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let acl_table = vec![
            AclTable {
                name: "Apple".to_string(),
                permission: "N".to_string(),
            },
            AclTable {
                name: "Banana".to_string(),
                permission: "Y".to_string(),
            },
        ];
        let engine = HelloAclSenderEngine {
            node,
            indicator: Default::default(),
            outstanding_req_pool,
            config,
            acl_table,
        };
        Ok(engine)
    }
}

impl HelloAclSenderEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut work = 0;
            // check input queue, ~100ns
            loop {
                match self.check_input_queue()? {
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
fn materialize(msg: &RpcMessageTx) -> Box<hello::HelloRequest> {
    let req_ptr = Unique::new(msg.addr_backend as *mut hello::HelloRequest).unwrap();
    let req = unsafe { req_ptr.as_ref() };
    // returns a private_req
    Box::new(req.clone())
}

#[inline]
fn nocopy_materialize(msg: &RpcMessageTx) -> &hello::HelloRequest {
    let req_ptr = msg.addr_backend as *mut hello::HelloRequest;
    let req = unsafe { req_ptr.as_ref().unwrap() };
    // returns a private_req
    return req;
}

fn hello_request_name_readonly(req: &hello::HelloRequest) -> &[u8] {
    let buf = &req.name as &[u8];
    buf
}

impl HelloAclSenderEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        let req = nocopy_materialize(&msg);
                        let conn_id = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.conn_id;
                        let call_id = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.call_id;
                        let rpc_id = RpcId::new(conn_id, call_id);
                        for acl_rec in &self.acl_table {
                            if String::from_utf8_lossy(&req.name) == acl_rec.name {
                                if acl_rec.permission == "N" {
                                    let error = EngineRxMessage::Ack(
                                        rpc_id,
                                        TransportStatus::Error(unsafe {
                                            NonZeroU32::new_unchecked(403)
                                        }),
                                    );
                                    self.rx_outputs()[0].send(error)?;
                                } else {
                                    let raw_ptr: *const hello::HelloRequest = req;
                                    let new_msg = RpcMessageTx {
                                        meta_buf_ptr: msg.meta_buf_ptr,
                                        addr_backend: raw_ptr.addr(),
                                    };
                                    self.tx_outputs()[0]
                                        .send(EngineTxMessage::RpcMessage(new_msg))?;
                                }
                                break;
                            }
                        }
                    }
                    // XXX TODO(cjr): it is best not to reorder the message
                    m => self.tx_outputs()[0].send(m)?,
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
