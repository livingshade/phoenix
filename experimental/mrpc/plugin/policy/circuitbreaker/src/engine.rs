use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use phoenix_api::rpc::{RpcId, StatusCode, TransportStatus};
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::num::NonZeroU32;
use std::os::unix::ucred::UCred;
use std::pin::Pin;
use std::ptr::Unique;

use phoenix_api_policy_circuitbreaker::control_plane;

use phoenix_common::engine::datapath::message::{
    EngineRxMessage, EngineTxMessage, RpcMessageGeneral,
};

use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::log;
use phoenix_common::module::Version;

use super::DatapathError;
use crate::config::circuitbreakerConfig;
use phoenix_common::engine::datapath::{RpcMessageRx, RpcMessageTx};
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use chrono::prelude::*;
use itertools::iproduct;
use minstant::Instant;
use rand::Rng;
use std::collections::HashMap;

pub mod hello {
    include!("proto.rs");
}

pub fn meta_status_readonly_tx() -> &'static str {
    "success"
}
pub fn meta_status_readonly_rx(msg: &RpcMessageRx) -> &'static str {
    let meta: &phoenix_api::rpc::MessageMeta = unsafe { &*msg.meta.as_ptr() };
    if meta.status_code == StatusCode::Success {
        "success"
    } else {
        "failure"
    }
}

fn hello_HelloRequest_name_readonly(req: &hello::HelloRequest) -> String {
    let buf = &req.name as &[u8];
    String::from_utf8_lossy(buf).to_string().clone()
}

fn hello_HelloReply_message_readonly(req: &hello::HelloReply) -> String {
    let buf = &req.message as &[u8];
    String::from_utf8_lossy(buf).to_string().clone()
}

fn hello_HelloRequest_name_modify(req: &mut hello::HelloRequest, value: &[u8]) {
    assert!(req.name.len() <= value.len());
    for i in 0..req.name.len() {
        req.name[i] = value[i];
    }
}

fn hello_HelloReply_message_modify(req: &mut hello::HelloReply, value: &[u8]) {
    assert!(req.message.len() <= value.len());
    for i in 0..req.message.len() {
        req.message[i] = value[i];
    }
}

pub(crate) struct circuitbreakerEngine {
    pub(crate) node: DataPathNode,
    pub(crate) indicator: Indicator,
    pub(crate) config: circuitbreakerConfig,
    pub(crate) max_concurrent_req: i32,
    pub(crate) pending_req: i32,
    pub(crate) drop_count: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for circuitbreakerEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "circuitbreakerEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig() => {
                self.config = circuitbreakerConfig {};
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(circuitbreakerEngine, node);

impl Decompose for circuitbreakerEngine {
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
        let mut collections = ResourceCollection::with_capacity(4);
        collections.insert("config".to_string(), Box::new(engine.config));
        (collections, engine.node)
    }
}

impl circuitbreakerEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<circuitbreakerConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let mut max_concurrent_req = 10;
        let mut pending_req = 0;
        let mut drop_count = 0;

        let engine = circuitbreakerEngine {
            node,
            indicator: Default::default(),
            config,
            max_concurrent_req,
            pending_req,
            drop_count,
        };
        Ok(engine)
    }
}

impl circuitbreakerEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut work = 0;
            loop {
                match self.check_input_queue()? {
                    Progress(0) => break,
                    Progress(n) => work += n,
                    Status::Disconnected => return Ok(()),
                }
            }
            self.indicator.set_nwork(work);
            future::yield_now().await;
        }
    }
}

#[inline]
fn materialize_nocopy_tx(msg: &RpcMessageTx) -> &hello::HelloRequest {
    let req_ptr = msg.addr_backend as *mut hello::HelloRequest;
    let req = unsafe { req_ptr.as_ref().unwrap() };
    return req;
}

#[inline]
fn materialize_nocopy_mutable_tx(msg: &RpcMessageTx) -> &mut hello::HelloRequest {
    let req_ptr = msg.addr_backend as *mut hello::HelloRequest;
    let req = unsafe { req_ptr.as_mut().unwrap() };
    return req;
}

#[inline]
fn materialize_nocopy_rx(msg: &RpcMessageRx) -> &hello::HelloRequest {
    let req_ptr = msg.addr_backend as *mut hello::HelloRequest;
    let req = unsafe { req_ptr.as_ref().unwrap() };
    return req;
}

#[inline]
fn materialize_nocopy_mutable_rx(msg: &RpcMessageRx) -> &mut hello::HelloRequest {
    let mut req_ptr = msg.addr_backend as *mut hello::HelloRequest;
    let req = unsafe { req_ptr.as_mut().unwrap() };
    return req;
}

impl circuitbreakerEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        match (self.pending_req <= self.max_concurrent_req) {
                            true => {
                                self.pending_req = (self.pending_req + 1) as i32;

                                let inner_gen = EngineTxMessage::RpcMessage(RpcMessageTx {
                                    meta_buf_ptr: msg.meta_buf_ptr.clone(),
                                    addr_backend: msg.addr_backend,
                                });
                                self.tx_outputs()[0].send(inner_gen)?;
                            }
                            false => {
                                self.drop_count = (self.drop_count + 1) as i32;

                                let inner_gen = EngineRxMessage::Ack(
                                    RpcId::new(
                                        unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.conn_id,
                                        unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.call_id,
                                    ),
                                    TransportStatus::Error(unsafe {
                                        NonZeroU32::new_unchecked(403)
                                    }),
                                );
                                self.rx_outputs()[0].send(inner_gen)?;
                            }
                        }
                    }
                    m => self.tx_outputs()[0].send(m)?,
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return Ok(Status::Disconnected);
            }
        }

        match self.rx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineRxMessage::Ack(rpc_id, status) => {
                        self.rx_outputs()[0].send(EngineRxMessage::Ack(rpc_id, status))?;
                    }
                    EngineRxMessage::RpcMessage(msg) => {
                        let ptr = msg.meta.as_ptr();
                        if unsafe { (*ptr).status_code } != StatusCode::Success {
                            self.rx_outputs()[0].send(EngineRxMessage::RpcMessage((msg)))?;
                        } else {
                            self.pending_req = (self.pending_req - 1) as i32;
                            let inner_gen = EngineRxMessage::RpcMessage(msg);
                            self.rx_outputs()[0].send(inner_gen)?;
                        }
                    }
                    m => self.rx_outputs()[0].send(m)?,
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return Ok(Status::Disconnected);
            }
        }
        Ok(Progress(0))
    }
}
