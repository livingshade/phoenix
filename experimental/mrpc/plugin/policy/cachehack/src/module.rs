use anyhow::{bail, Result};
use nix::unistd::Pid;
use phoenix_api::rpc::{RpcId, StatusCode, TransportStatus};
use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::cachehackEngine;
use crate::config::cachehackConfig;

use chrono::prelude::*;
use itertools::iproduct;
use minstant::Instant;
use rand::Rng;
use std::collections::HashMap;

pub(crate) struct cachehackEngineBuilder {
    node: DataPathNode,
    config: cachehackConfig,
}

impl cachehackEngineBuilder {
    fn new(node: DataPathNode, config: cachehackConfig) -> Self {
        cachehackEngineBuilder { node, config }
    }
    // TODO! LogFile
    fn build(self) -> Result<cachehackEngine> {
        let mut cache = HashMap::new();
        Ok(cachehackEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            cache,
        })
    }
}

pub struct cachehackAddon {
    config: cachehackConfig,
}

impl cachehackAddon {
    pub const cachehack_ENGINE: EngineType = EngineType("cachehackEngine");
    pub const ENGINES: &'static [EngineType] = &[cachehackAddon::cachehack_ENGINE];
}

impl cachehackAddon {
    pub fn new(config: cachehackConfig) -> Self {
        cachehackAddon { config }
    }
}

impl PhoenixAddon for cachehackAddon {
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
        cachehackAddon::ENGINES
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
        if ty != cachehackAddon::cachehack_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = cachehackEngineBuilder::new(node, self.config);
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
        if ty != cachehackAddon::cachehack_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = cachehackEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
