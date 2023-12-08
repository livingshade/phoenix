use chrono::{Datelike, Timelike, Utc};
use phoenix_common::log;
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenencryptclientConfig {}

impl GenencryptclientConfig {
    /// Get config from toml file
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(config.unwrap_or(""))?;
        Ok(config)
    }
}

pub fn create_log_file() -> std::fs::File {
    std::fs::create_dir_all("/tmp/phoenix/log").expect("mkdir failed");
    let now = Utc::now();
    let date_string = format!(
        "{}-{}-{}-{}-{}-{}",
        now.year(),
        now.month(),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    );
    let file_name = format!("/tmp/phoenix/log/logging_engine_{}.log", date_string);
    ///log::info!("create log file {}", file_name);
    let log_file = std::fs::File::create(file_name).expect("create file failed");
    log_file
}
