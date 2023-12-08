use anyhow::{bail, Result};
use nix::unistd::Pid;
use phoenix_api::rpc::{RpcId, StatusCode, TransportStatus};
use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::GenencryptclientEngine;
use crate::config::{create_log_file, GenencryptclientConfig};

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

pub(crate) struct GenencryptclientEngineBuilder {
    node: DataPathNode,
    config: GenencryptclientConfig,
}

impl GenencryptclientEngineBuilder {
    fn new(node: DataPathNode, config: GenencryptclientConfig) -> Self {
        GenencryptclientEngineBuilder { node, config }
    }
    // TODO! LogFile
    fn build(self) -> Result<GenencryptclientEngine> {
        let mut password = "123456".to_string();
        Ok(GenencryptclientEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            password,
        })
    }
}

pub struct GenencryptclientAddon {
    config: GenencryptclientConfig,
}

impl GenencryptclientAddon {
    pub const GENENCRYPTCLIENT_ENGINE: EngineType = EngineType("GenencryptclientEngine");
    pub const ENGINES: &'static [EngineType] = &[GenencryptclientAddon::GENENCRYPTCLIENT_ENGINE];
}

impl GenencryptclientAddon {
    pub fn new(config: GenencryptclientConfig) -> Self {
        GenencryptclientAddon { config }
    }
}

impl PhoenixAddon for GenencryptclientAddon {
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
        GenencryptclientAddon::ENGINES
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
        if ty != GenencryptclientAddon::GENENCRYPTCLIENT_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = GenencryptclientEngineBuilder::new(node, self.config);
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
        if ty != GenencryptclientAddon::GENENCRYPTCLIENT_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = GenencryptclientEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
