//! The in-memory shape a `ConfigSource` resolves a config file into. Kept in
//! `core` so the port returns domain types, not the TOML wire schema.

use super::job::Job;
use super::process::ProcessSpec;

/// A fully-parsed, validated configuration snapshot.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadedConfig {
    pub processes: Vec<ProcessSpec>,
    pub jobs: Vec<Job>,
}
