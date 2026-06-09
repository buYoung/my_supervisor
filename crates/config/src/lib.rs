//! `my-supervisor-config` — the `ConfigSource` reference implementation. Parses
//! TOML via `shared::config` and maps it onto `core` domain types.

mod convert;

use std::path::PathBuf;

use async_trait::async_trait;

use my_supervisor_core::domain::LoadedConfig;
use my_supervisor_core::ports::error::ConfigError;
use my_supervisor_core::ports::ConfigSource;
use my_supervisor_shared::config::FileConfig;

/// Loads configuration from a TOML file on disk.
pub struct TomlConfigSource {
    path: PathBuf,
}

impl TomlConfigSource {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        TomlConfigSource { path: path.into() }
    }

    fn parse(contents: &str) -> Result<LoadedConfig, ConfigError> {
        let file: FileConfig =
            toml::from_str(contents).map_err(|e| ConfigError::Invalid(e.to_string()))?;
        Ok(LoadedConfig {
            processes: file.processes.into_iter().map(convert::process_spec).collect(),
            jobs: file.jobs.into_iter().map(convert::job).collect(),
        })
    }
}

#[async_trait]
impl ConfigSource for TomlConfigSource {
    async fn load(&self) -> Result<LoadedConfig, ConfigError> {
        // An absent config file is a valid empty configuration.
        match tokio::fs::read_to_string(&self.path).await {
            Ok(contents) => Self::parse(&contents),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(LoadedConfig::default()),
            Err(e) => Err(ConfigError::Io(e.to_string())),
        }
    }

    fn path(&self) -> PathBuf {
        self.path.clone()
    }
}
