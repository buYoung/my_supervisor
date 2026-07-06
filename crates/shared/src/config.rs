//! TOML config-file schema. Reuses the REST DTOs for `[[process]]` / `[[job]]`
//! entries so the file format and the wire format stay in lock-step.

use serde::{Deserialize, Serialize};

use crate::api::{JobConfigDto, ProcessConfigDto};

/// Root of the config file (`config.toml`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileConfig {
    #[serde(default, rename = "process")]
    pub processes: Vec<ProcessConfigDto>,
    #[serde(default, rename = "job")]
    pub jobs: Vec<JobConfigDto>,
}
