use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Result};
use uuid::Uuid;

use ipc::customer::ShmCustomer;
use phoenix_api::engine::SchedulingMode;
use phoenix_api_mrpc::control_plane::Setting;
use phoenix_api_mrpc::control_plane::TransportType;
use phoenix_api_mrpc::{cmd, dp};

use phoenix_common::engine::datapath::meta_pool::MetaBufferPool;
use phoenix_common::engine::datapath::node::{ChannelDescriptor, DataPathNode};
use phoenix_common::engine::{Engine, EnginePair, EngineType};
use phoenix_common::log;
use phoenix_common::module::{
    ModuleCollection, ModuleDowncast, NewEngineRequest, PhoenixModule, Service, ServiceInfo,
    Version,
};
use phoenix_common::state_mgr::{Pid, SharedStateManager};
use phoenix_common::storage::{get_default_prefix, ResourceCollection, SharedStorage};
use phoenix_common::PhoenixResult;

use crate::config::MrpcLBConfig;

use super::engine::MrpcLBEngine;
use super::state::{Shared, State};

pub type CustomerType =
    ShmCustomer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>;

pub(crate) struct MrpcLBEngineBuilder {
    customer: CustomerType,
    _client_pid: Pid,
    mode: SchedulingMode,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Command>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Completion>,
    node: DataPathNode,
    serializer_build_cache: PathBuf,
    shared: Arc<Shared>,
}

impl MrpcLBEngineBuilder {
    #[allow(clippy::too_many_arguments)]
    fn new(
        customer: CustomerType,
        client_pid: Pid,
        mode: SchedulingMode,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Command>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Completion>,
        node: DataPathNode,
        serializer_build_cache: PathBuf,
        shared: Arc<Shared>,
    ) -> Self {
        MrpcLBEngineBuilder {
            customer,
            cmd_tx,
            cmd_rx,
            node,
            _client_pid: client_pid,
            mode,
            serializer_build_cache,
            shared,
        }
    }

    fn build(self) -> Result<MrpcLBEngine> {
        const META_BUFFER_POOL_CAP: usize = 128;
        const BUF_LEN: usize = 32;

        let state = State::new(self.shared);

        Ok(MrpcLBEngine {
            _state: state,
            customer: self.customer,
            cmd_tx: self.cmd_tx,
            cmd_rx: self.cmd_rx,
            node: self.node,
            meta_buf_pool: MetaBufferPool::new(META_BUFFER_POOL_CAP),
            _mode: self.mode,
            dispatch_build_cache: self.serializer_build_cache,
            transport_type: Some(TransportType::Tcp),
            indicator: Default::default(),
            wr_read_buffer: Vec::with_capacity(BUF_LEN),
        })
    }
}

pub struct MrpcLBModule {
    config: MrpcLBConfig,
    pub state_mgr: SharedStateManager<Shared>,
}

impl MrpcLBModule {
    pub const MRPCLB_ENGINE: EngineType = EngineType("MrpcLBEngine");
    pub const LB_ENGINE: EngineType = EngineType("LoadBalancerEngine");
    pub const ENGINES: &'static [EngineType] = &[MrpcLBModule::MRPCLB_ENGINE];
    pub const DEPENDENCIES: &'static [EnginePair] = &[
        (
            MrpcLBModule::MRPCLB_ENGINE,
            EngineType("LoadBalancerEngine"),
        ),
        (
            EngineType("LoadBalancerEngine"),
            EngineType("RpcAdapterEngine"),
        ),
    ];

    pub const SERVICE: Service = Service("MrpcLB");
    pub const TX_CHANNELS: &'static [ChannelDescriptor] = &[
        ChannelDescriptor(MrpcLBModule::MRPCLB_ENGINE, MrpcLBModule::LB_ENGINE, 0, 0),
        ChannelDescriptor(
            MrpcLBModule::LB_ENGINE,
            EngineType("RpcAdapterEngine"),
            0,
            0,
        ),
    ];
    pub const RX_CHANNELS: &'static [ChannelDescriptor] = &[
        ChannelDescriptor(
            EngineType("RpcAdapterEngine"),
            MrpcLBModule::LB_ENGINE,
            0,
            0,
        ),
        ChannelDescriptor(MrpcLBModule::LB_ENGINE, MrpcLBModule::MRPCLB_ENGINE, 0, 0),
    ];

    pub const TCP_DEPENDENCIES: &'static [EnginePair] = &[
        (
            MrpcLBModule::MRPCLB_ENGINE,
            EngineType("LoadBalancerEngine"),
        ),
        (
            EngineType("LoadBalancerEngine"),
            EngineType("TcpRpcAdapterEngine"),
        ),
    ];
    pub const TCP_TX_CHANNELS: &'static [ChannelDescriptor] = &[
        ChannelDescriptor(MrpcLBModule::MRPCLB_ENGINE, MrpcLBModule::LB_ENGINE, 0, 0),
        ChannelDescriptor(
            MrpcLBModule::LB_ENGINE,
            EngineType("TcpRpcAdapterEngine"),
            0,
            0,
        ),
    ];
    pub const TCP_RX_CHANNELS: &'static [ChannelDescriptor] = &[
        ChannelDescriptor(
            EngineType("TcpRpcAdapterEngine"),
            MrpcLBModule::LB_ENGINE,
            0,
            0,
        ),
        ChannelDescriptor(MrpcLBModule::LB_ENGINE, MrpcLBModule::MRPCLB_ENGINE, 0, 0),
    ];
}

impl MrpcLBModule {
    pub fn new(config: MrpcLBConfig) -> Self {
        MrpcLBModule {
            config,
            state_mgr: SharedStateManager::new(),
        }
    }

    // Returns build_cache if it's already an absolute path. Otherwise returns the path relative
    // to the engine's prefix.
    fn get_build_cache_directory(&self, engine_prefix: &PathBuf) -> PathBuf {
        let build_cache = self.config.build_cache.clone();
        if build_cache.is_absolute() {
            build_cache
        } else {
            engine_prefix.join(build_cache)
        }
    }
}

impl PhoenixModule for MrpcLBModule {
    fn service(&self) -> Option<ServiceInfo> {
        let service = if self.config.transport == TransportType::Tcp {
            let group = vec![
                Self::MRPCLB_ENGINE,
                Self::LB_ENGINE,
                EngineType("TcpRpcAdapterEngine"),
            ];
            ServiceInfo {
                service: MrpcLBModule::SERVICE,
                engine: MrpcLBModule::MRPCLB_ENGINE,
                tx_channels: MrpcLBModule::TCP_TX_CHANNELS,
                rx_channels: MrpcLBModule::TCP_RX_CHANNELS,
                scheduling_groups: vec![group],
            }
        } else {
            let group = vec![
                Self::MRPCLB_ENGINE,
                Self::LB_ENGINE,
                EngineType("RpcAdapterEngine"),
            ];
            ServiceInfo {
                service: MrpcLBModule::SERVICE,
                engine: MrpcLBModule::MRPCLB_ENGINE,
                tx_channels: MrpcLBModule::TX_CHANNELS,
                rx_channels: MrpcLBModule::RX_CHANNELS,
                scheduling_groups: vec![group],
            }
        };
        Some(service)
    }

    fn engines(&self) -> &[EngineType] {
        MrpcLBModule::ENGINES
    }

    fn dependencies(&self) -> &[EnginePair] {
        if self.config.transport == TransportType::Tcp {
            MrpcLBModule::TCP_DEPENDENCIES
        } else {
            MrpcLBModule::DEPENDENCIES
        }
    }

    fn check_compatibility(&self, _prev: Option<&Version>, _curr: &HashMap<&str, Version>) -> bool {
        true
    }

    fn decompose(self: Box<Self>) -> ResourceCollection {
        let module = *self;
        let mut collections = ResourceCollection::new();
        collections.insert("state_mgr".to_string(), Box::new(module.state_mgr));
        collections.insert("config".to_string(), Box::new(module.config));
        collections
    }

    fn migrate(&mut self, prev_module: Box<dyn PhoenixModule>) {
        // NOTE(wyj): we may better call decompose here
        let prev_concrete = unsafe { *prev_module.downcast_unchecked::<Self>() };
        self.state_mgr = prev_concrete.state_mgr;
    }

    fn create_engine(
        &mut self,
        ty: EngineType,
        request: NewEngineRequest,
        shared: &mut SharedStorage,
        global: &mut ResourceCollection,
        node: DataPathNode,
        _plugged: &ModuleCollection,
    ) -> PhoenixResult<Option<Box<dyn Engine>>> {
        if ty != MrpcLBModule::MRPCLB_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }
        if let NewEngineRequest::Service {
            sock,
            client_path,
            mode,
            cred,
            config_string,
        } = request
        {
            // generate a path and bind a unix domain socket to it
            let uuid = Uuid::new_v4();
            let instance_name = format!("{}-{}.sock", self.config.engine_basename, uuid);

            // use the phoenix_prefix if not otherwise specified
            let phoenix_prefix = get_default_prefix(global)?;
            let engine_prefix = self.config.prefix.as_ref().unwrap_or(phoenix_prefix);
            let engine_path = engine_prefix.join(instance_name);

            // get the directory of build cache
            let build_cache = self.get_build_cache_directory(engine_prefix);

            // create customer stub
            let customer = ShmCustomer::accept(sock, client_path, mode, engine_path)?;

            let client_pid = Pid::from_raw(cred.pid.unwrap());
            let shared_state = self.state_mgr.get_or_create(client_pid)?;

            let setting = if let Some(config_string) = config_string {
                serde_json::from_str(&config_string)?
            } else {
                Setting {
                    transport: self.config.transport,
                    nic_index: self.config.nic_index,
                    core_id: None,
                    module_config: None,
                }
            };
            log::debug!("mRPCLB service setting: {:?}", setting);

            let engine_type = match setting.transport {
                TransportType::Tcp => EngineType("TcpRpcAdapterEngine"),
                TransportType::Rdma => EngineType("RpcAdapterEngine"),
            };

            // obtain senders/receivers of command queues with RpcAdapterEngine
            // the sender/receiver ends are already created,
            // as the RpcAdapterEngine is built first
            // according to the topological order
            let cmd_tx = shared.command_path.get_sender(&MrpcLBModule::LB_ENGINE)?;
            let cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Completion> =
                shared.command_path.get_receiver(&MrpcLBModule::LB_ENGINE)?;

            let builder = MrpcLBEngineBuilder::new(
                customer,
                client_pid,
                mode,
                cmd_tx,
                cmd_rx,
                node,
                build_cache,
                shared_state,
                // TODO(cjr): store the setting, not necessary now.
            );
            let engine = builder.build()?;

            Ok(Some(Box::new(engine)))
        } else {
            bail!("invalid request type");
        }
    }

    fn restore_engine(
        &mut self,
        ty: EngineType,
        local: ResourceCollection,
        shared: &mut SharedStorage,
        global: &mut ResourceCollection,
        node: DataPathNode,
        plugged: &ModuleCollection,
        prev_version: Version,
    ) -> PhoenixResult<Box<dyn Engine>> {
        if ty != MrpcLBModule::MRPCLB_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }
        let engine = MrpcLBEngine::restore(local, shared, global, node, plugged, prev_version)?;
        Ok(Box::new(engine))
    }
}
