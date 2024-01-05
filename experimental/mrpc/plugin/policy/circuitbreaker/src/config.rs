use chrono::{Datelike, Timelike, Utc};
use phoenix_common::log;
use serde::{Deserialize, Serialize};

use chrono::prelude::*;
use itertools::iproduct;
use minstant::Instant;
use rand::Rng;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct circuitbreakerConfig {}

impl circuitbreakerConfig {
    /// Get config from toml file
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(config.unwrap_or(""))?;
        Ok(config)
    }
}
