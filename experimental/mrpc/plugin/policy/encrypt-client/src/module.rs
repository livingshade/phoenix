use anyhow::{bail, Result};
use nix::unistd::Pid;
use phoenix_api::rpc::{RpcId, StatusCode, TransportStatus};
use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::EncryptClientEngine;
use crate::config::{create_log_file, EncryptClientConfig};

use chrono::prelude::*;
use itertools::iproduct;
use minstant::Instant;
use rand::Rng;
use std::collections::HashMap;

pub(crate) struct EncryptClientEngineBuilder {
    node: DataPathNode,
    config: EncryptClientConfig,
}

impl EncryptClientEngineBuilder {
    fn new(node: DataPathNode, config: EncryptClientConfig) -> Self {
        EncryptClientEngineBuilder { node, config }
    }
    // TODO! LogFile
    fn build(self) -> Result<EncryptClientEngine> {
        let mut password = "123456".to_string();
        Ok(EncryptClientEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            password,
        })
    }
}

pub struct EncryptClientAddon {
    config: EncryptClientConfig,
}

impl EncryptClientAddon {
    pub const GENENCRYPTCLIENT_ENGINE: EngineType = EngineType("EncryptClientEngine");
    pub const ENGINES: &'static [EngineType] = &[EncryptClientAddon::GENENCRYPTCLIENT_ENGINE];
}

impl EncryptClientAddon {
    pub fn new(config: EncryptClientConfig) -> Self {
        EncryptClientAddon { config }
    }
}

impl PhoenixAddon for EncryptClientAddon {
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
        EncryptClientAddon::ENGINES
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
        if ty != EncryptClientAddon::GENENCRYPTCLIENT_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = EncryptClientEngineBuilder::new(node, self.config);
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
        if ty != EncryptClientAddon::GENENCRYPTCLIENT_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = EncryptClientEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
