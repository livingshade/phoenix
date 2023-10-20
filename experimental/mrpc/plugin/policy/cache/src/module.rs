use std::collections::BTreeMap;

use anyhow::{bail, Result};
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::meta_pool::MetaBufferPool;
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::CacheEngine;
use crate::config::CacheConfig;

pub mod cache {
    // The string specified here must match the proto package name
    include!("cache.rs");
}

pub(crate) struct CacheEngineBuilder {
    node: DataPathNode,
    config: CacheConfig,
}

impl CacheEngineBuilder {
    fn new(node: DataPathNode, config: CacheConfig) -> Self {
        CacheEngineBuilder { node, config }
    }

    fn build(self) -> Result<CacheEngine> {
        let META_POOL_CAP = 1024;
        Ok(CacheEngine {
            node: self.node,
            indicator: Default::default(),
            buffer: BTreeMap::new(),
            meta_pool: MetaBufferPool::new(META_POOL_CAP),
            config: self.config,
        })
    }
}

pub struct CacheAddon {
    config: CacheConfig,
}

impl CacheAddon {
    pub const CACHE_ENGINE: EngineType = EngineType("CacheEngine");
    pub const ENGINES: &'static [EngineType] = &[CacheAddon::CACHE_ENGINE];
}

impl CacheAddon {
    pub fn new(config: CacheConfig) -> Self {
        CacheAddon { config }
    }
}

impl PhoenixAddon for CacheAddon {
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
        CacheAddon::ENGINES
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
        if ty != CacheAddon::CACHE_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = CacheEngineBuilder::new(node, self.config);
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
        if ty != CacheAddon::CACHE_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = CacheEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
