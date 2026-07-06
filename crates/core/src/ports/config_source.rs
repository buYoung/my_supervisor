//! `ConfigSource` — loads and validates the TOML config into domain types.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::domain::LoadedConfig;
use crate::ports::error::ConfigError;

#[async_trait]
pub trait ConfigSource: Send + Sync {
    /// Load and validate the current config.
    async fn load(&self) -> Result<LoadedConfig, ConfigError>;
    /// The config file path this source reads.
    fn path(&self) -> PathBuf;
}
