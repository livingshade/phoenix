use fnv::FnvHashMap;
use std::{
    ops::Deref,
    rc::{Rc, Weak},
};
use thiserror::Error;

use crate::{
    engine::{CommandSignal, Engine, EngineType, OpaqueParam},
    PhoenixResult,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("Resource not found")]
    NotFound,
}

pub struct CommandExecutor {
    engines: FnvHashMap<EngineType, Box<dyn Engine>>,
}

impl CommandExecutor {
    pub fn new() -> Self {
        CommandExecutor {
            engines: FnvHashMap::default(),
        }
    }

    pub fn add_engine(&mut self, etype: EngineType, engine: Box<dyn Engine>) {
        self.engines.insert(etype, engine);
    }

    pub fn execute(&mut self, signal: CommandSignal) -> PhoenixResult<()> {
        for engine in self.engines.values_mut() {
            engine.handle_command(signal.clone())?;
        }
        Ok(())
    }
}
