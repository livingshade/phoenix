use anyhow::{bail, Result};
use nix::unistd::Pid;
use phoenix_api::rpc::{RpcId, StatusCode, TransportStatus};
use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::circuitbreakerEngine;
use crate::config::circuitbreakerConfig;

use chrono::prelude::*;
use itertools::iproduct;
use minstant::Instant;
use rand::Rng;
use std::collections::HashMap;

pub(crate) struct circuitbreakerEngineBuilder {
    node: DataPathNode,
    config: circuitbreakerConfig,
}

impl circuitbreakerEngineBuilder {
    fn new(node: DataPathNode, config: circuitbreakerConfig) -> Self {
        circuitbreakerEngineBuilder { node, config }
    }
    // TODO! LogFile
    fn build(self) -> Result<circuitbreakerEngine> {
        let mut max_concurrent_req = 10;
        let mut pending_req = 0;
        let mut drop_count = 0;

        Ok(circuitbreakerEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            max_concurrent_req,
            pending_req,
            drop_count,
        })
    }
}

pub struct circuitbreakerAddon {
    config: circuitbreakerConfig,
}

impl circuitbreakerAddon {
    pub const circuitbreaker_ENGINE: EngineType = EngineType("circuitbreakerEngine");
    pub const ENGINES: &'static [EngineType] = &[circuitbreakerAddon::circuitbreaker_ENGINE];
}

impl circuitbreakerAddon {
    pub fn new(config: circuitbreakerConfig) -> Self {
        circuitbreakerAddon { config }
    }
}

impl PhoenixAddon for circuitbreakerAddon {
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
        circuitbreakerAddon::ENGINES
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
        if ty != circuitbreakerAddon::circuitbreaker_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = circuitbreakerEngineBuilder::new(node, self.config);
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
        if ty != circuitbreakerAddon::circuitbreaker_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = circuitbreakerEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
