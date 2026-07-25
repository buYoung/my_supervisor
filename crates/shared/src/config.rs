//! TOML config-file schema. Reuses the REST DTOs for `[[process]]` / `[[job]]`
//! entries so the file format and the wire format stay in lock-step.

use serde::{Deserialize, Serialize};

use crate::api::{ConfigApplyModeDto, JobConfigDto, ProcessConfigDto};

/// Root of the config file (`config.toml`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileConfig {
    #[serde(default, rename = "process")]
    pub processes: Vec<ProcessConfigDto>,
    #[serde(default, rename = "job")]
    pub jobs: Vec<JobConfigDto>,
}

/// Request body shared by config validation and application endpoints.
/// `dry_run` is accepted on both paths so a caller can use one payload shape
/// while explicitly documenting its intent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigApplyRequestDto {
    #[serde(default)]
    pub mode: ConfigApplyModeDto,
    #[serde(default)]
    pub dry_run: bool,
    pub config: FileConfig,
}
