use anyhow::{bail, Result};
use nix::unistd::Pid;
use phoenix_api::rpc::{RpcId, StatusCode, TransportStatus};
use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::meta_pool::MetaBufferPool;
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::DecryptServerEngine;
use crate::config::{create_log_file, DecryptServerConfig};

use chrono::prelude::*;
use itertools::iproduct;
use minstant::Instant;
use rand::Rng;
use std::collections::HashMap;

use crate::engine::{
    meta_id_readonly_rx, meta_id_readonly_tx, meta_status_readonly_rx, meta_status_readonly_tx,
    Gen_current_timestamp, Gen_decrypt, Gen_encrypt, Gen_min_f64, Gen_min_u64, Gen_random_f32,
    Gen_time_difference, Gen_update_window,
};

pub(crate) struct DecryptServerEngineBuilder {
    node: DataPathNode,
    config: DecryptServerConfig,
}

impl DecryptServerEngineBuilder {
    fn new(node: DataPathNode, config: DecryptServerConfig) -> Self {
        DecryptServerEngineBuilder { node, config }
    }
    // TODO! LogFile
    fn build(self) -> Result<DecryptServerEngine> {
        let mut password = "123456".to_string();
        const META_BUFFER_POOL_CAP: usize = 128;
        Ok(DecryptServerEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            meta_buf_pool: MetaBufferPool::new(META_BUFFER_POOL_CAP),
            password,
        })
    }
}

pub struct DecryptServerAddon {
    config: DecryptServerConfig,
}

impl DecryptServerAddon {
    pub const GENDECRYPTSERVER_ENGINE: EngineType = EngineType("DecryptServerEngine");
    pub const ENGINES: &'static [EngineType] = &[DecryptServerAddon::GENDECRYPTSERVER_ENGINE];
}

impl DecryptServerAddon {
    pub fn new(config: DecryptServerConfig) -> Self {
        DecryptServerAddon { config }
    }
}

impl PhoenixAddon for DecryptServerAddon {
    fn check_compatibility(&self, _prev: Option<&Version>) -> bool {
        true
    }

    fn decompose(self: Box<Self>) -> ResourceCollection {
        let addon = *self;
        let mut collections = ResourceCollection::new();
        collections.insert("config".to_string(), Box::new(addon.config));
        collections
    }

    #[inline]
    fn migrate(&mut self, _prev_addon: Box<dyn PhoenixAddon>) {}

    fn engines(&self) -> &[EngineType] {
        DecryptServerAddon::ENGINES
    }

    fn update_config(&mut self, config: &str) -> Result<()> {
        self.config = toml::from_str(config)?;
        Ok(())
    }

    fn create_engine(
        &mut self,
        ty: EngineType,
        _pid: Pid,
        node: DataPathNode,
    ) -> Result<Box<dyn Engine>> {
        if ty != DecryptServerAddon::GENDECRYPTSERVER_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = DecryptServerEngineBuilder::new(node, self.config);
        let engine = builder.build()?;
        Ok(Box::new(engine))
    }

    fn restore_engine(
        &mut self,
        ty: EngineType,
        local: ResourceCollection,
        node: DataPathNode,
        prev_version: Version,
    ) -> Result<Box<dyn Engine>> {
        if ty != DecryptServerAddon::GENDECRYPTSERVER_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = DecryptServerEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
