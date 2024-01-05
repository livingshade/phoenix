use anyhow::{bail, Result};
use nix::unistd::Pid;
use phoenix_api::rpc::{RpcId, StatusCode, TransportStatus};
use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::lbstickyhackEngine;
use crate::config::{create_log_file, lbstickyhackConfig};

use chrono::prelude::*;
use itertools::iproduct;
use minstant::Instant;
use rand::Rng;
use std::collections::HashMap;

pub(crate) struct lbstickyhackEngineBuilder {
    node: DataPathNode,
    config: lbstickyhackConfig,
}

impl lbstickyhackEngineBuilder {
    fn new(node: DataPathNode, config: lbstickyhackConfig) -> Self {
        lbstickyhackEngineBuilder { node, config }
    }
    // TODO! LogFile
    fn build(self) -> Result<lbstickyhackEngine> {
        let mut lb_tab = HashMap::new();
        Ok(lbstickyhackEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            lb_tab,
        })
    }
}

pub struct lbstickyhackAddon {
    config: lbstickyhackConfig,
}

impl lbstickyhackAddon {
    pub const lbstickyhack_ENGINE: EngineType = EngineType("lbstickyhackEngine");
    pub const ENGINES: &'static [EngineType] = &[lbstickyhackAddon::lbstickyhack_ENGINE];
}

impl lbstickyhackAddon {
    pub fn new(config: lbstickyhackConfig) -> Self {
        lbstickyhackAddon { config }
    }
}

impl PhoenixAddon for lbstickyhackAddon {
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
        lbstickyhackAddon::ENGINES
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
        if ty != lbstickyhackAddon::lbstickyhack_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = lbstickyhackEngineBuilder::new(node, self.config);
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
        if ty != lbstickyhackAddon::lbstickyhack_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = lbstickyhackEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
