#![feature(peer_credentials_unix_socket)]
#![feature(ptr_internals)]
#![feature(strict_provenance)]
use thiserror::Error;

use chrono::prelude::*;
use itertools::iproduct;
use minstant::Instant;
use rand::Rng;
use std::collections::HashMap;

pub use phoenix_common::{InitFnResult, PhoenixAddon};

pub mod config;
pub(crate) mod engine;
pub mod module;

#[derive(Error, Debug)]
pub(crate) enum DatapathError {
    #[error("Internal queue send error")]
    InternalQueueSend,
}

use phoenix_common::engine::datapath::SendError;
impl<T> From<SendError<T>> for DatapathError {
    fn from(_other: SendError<T>) -> Self {
        DatapathError::InternalQueueSend
    }
}

use crate::config::DecryptServerConfig;
use crate::module::DecryptServerAddon;

#[no_mangle]
pub fn init_addon(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixAddon>> {
    let config = DecryptServerConfig::new(config_string)?;
    let addon = DecryptServerAddon::new(config);
    Ok(Box::new(addon))
}
