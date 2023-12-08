use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::num::NonZeroU32;
use std::os::unix::ucred::UCred;
use std::pin::Pin;
use std::ptr::Unique;

use phoenix_api_policy_decrypt_server::control_plane;

use phoenix_api::rpc::{RpcId, StatusCode, TransportStatus};
use phoenix_common::engine::datapath::message::{
    EngineRxMessage, EngineTxMessage, RpcMessageGeneral,
};
use phoenix_common::engine::datapath::meta_pool::MetaBufferPool;
use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::log;
use phoenix_common::module::Version;

use phoenix_common::engine::datapath::{RpcMessageRx, RpcMessageTx};
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::{create_log_file, DecryptServerConfig};

use chrono::prelude::*;
use itertools::iproduct;
use minstant::Instant;
use rand::Rng;
use std::collections::HashMap;

pub mod hello {
    include!("proto.rs");
}

pub fn Gen_encrypt(a: &str, b: &str) -> String {
    a.to_string()
}
pub fn Gen_decrypt(a: &str, b: &str) -> String {
    a.to_string()
}
pub fn Gen_update_window(a: u64, b: u64) -> u64 {
    a.max(b)
}
pub fn Gen_current_timestamp() -> Instant {
    Instant::now()
}
pub fn Gen_time_difference(a: Instant, b: Instant) -> f32 {
    (a - b).as_secs_f64() as f32
}
pub fn Gen_random_f32(l: f32, r: f32) -> f32 {
    rand::random::<f32>()
}
pub fn Gen_min_u64(a: u64, b: u64) -> u64 {
    a.min(b)
}
pub fn Gen_min_f64(a: f64, b: f64) -> f64 {
    a.min(b)
}
pub fn meta_id_readonly_tx() -> u64 {
    0
}
pub fn meta_id_readonly_rx() -> u64 {
    0
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

pub(crate) struct DecryptServerEngine {
    pub(crate) node: DataPathNode,
    pub(crate) indicator: Indicator,
    pub(crate) config: DecryptServerConfig,
    pub(crate) meta_buf_pool: MetaBufferPool,
    pub(crate) password: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for DecryptServerEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "DecryptServerEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig() => {
                self.config = DecryptServerConfig {};
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(DecryptServerEngine, node);

impl Decompose for DecryptServerEngine {
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
        collections.insert("meta_buf_pool".to_string(), Box::new(engine.meta_buf_pool));
        (collections, engine.node)
    }
}

impl DecryptServerEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<DecryptServerConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let meta_buf_pool = *local
            .remove("meta_buf_pool")
            .unwrap()
            .downcast::<MetaBufferPool>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let mut password = "123456".to_string();
        let engine = DecryptServerEngine {
            node,
            indicator: Default::default(),
            config,
            meta_buf_pool,
            password,
        };
        Ok(engine)
    }
}

impl DecryptServerEngine {
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
    let req_ptr = msg.addr_backend as *mut hello::HelloRequest;
    let req = unsafe { req_ptr.as_mut().unwrap() };
    return req;
}

impl DecryptServerEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        let rpc_resp = materialize_nocopy_tx(&msg);
                        let rpc_resp_mut = materialize_nocopy_mutable_tx(&msg);

                        let inner_gen = EngineTxMessage::RpcMessage(msg);
                        self.tx_outputs()[0].send(inner_gen)?;
                    }
                    m => self.tx_outputs()[0].send(m)?,
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        match self.rx_inputs()[0].try_recv() {
            Ok(m) => {
                match m {
                    EngineRxMessage::Ack(rpc_id, _status) => {
                        if let Ok(()) = self.meta_buf_pool.release(rpc_id) {
                        } else {
                            self.rx_outputs()[0].send(m)?;
                        }
                    }
                    EngineRxMessage::RpcMessage(msg) => {
                        let rpc_req = materialize_nocopy_rx(&msg);
                        let rpc_req_mut = materialize_nocopy_mutable_rx(&msg);
                        let mut encrypted = hello_HelloRequest_name_readonly(&rpc_req);
                        let mut decrypted = Gen_decrypt(&encrypted, &self.password);
                        hello_HelloRequest_name_modify(rpc_req_mut, decrypted.as_bytes());

                        let inner_gen = EngineRxMessage::RpcMessage(RpcMessageRx {
                            meta: msg.meta.clone(),
                            addr_app: msg.addr_app,
                            addr_backend: msg.addr_backend,
                        });
                        self.rx_outputs()[0].send(inner_gen)?;
                    }
                    EngineRxMessage::RecvError(_, _) => {
                        self.rx_outputs()[0].send(m)?;
                    }
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        Ok(Progress(0))
    }
}
