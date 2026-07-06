//! `LifecycleController` — Direct-mode spawn / probe / reap. Each OS implements
//! the tied/detached strategy with its own kernel primitives (DD-018).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::domain::{ChildHandle, JobRunId, ProcessSpec};

/// Result of an aliveness probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aliveness {
    Alive,
    Dead,
}

/// Outcome of a transient (run-to-completion) execution used by `JobRunner`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransientOutcome {
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Error)]
pub enum SpawnError {
    #[error("working directory does not exist: {0}")]
    CwdMissing(String),
    #[error("failed to spawn '{name}': {message}")]
    Io { name: String, message: String },
}

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("probe failed: {0}")]
    Failed(String),
}

#[derive(Debug, Error)]
pub enum ReapError {
    #[error("reap failed: {0}")]
    Failed(String),
}

/// Direct-mode process control. SystemRegistered processes use
/// `ProcessServiceRegistrar` instead.
#[async_trait]
pub trait LifecycleController: Send + Sync {
    /// Spawn a child tied to the daemon (dies when the daemon exits).
    async fn spawn_tied(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError>;
    /// Spawn a child that survives daemon exit.
    async fn spawn_detached(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError>;
    /// Check whether a previously-spawned child is still alive.
    async fn probe_alive(&self, handle: &ChildHandle) -> Result<Aliveness, ProbeError>;
    /// Clean up the given handles during daemon shutdown.
    async fn reap_on_shutdown(&self, handles: &[ChildHandle]) -> Result<(), ReapError>;
    /// Spawn a transient process, capture its output to the run log, wait for
    /// exit, and report the outcome. Used by `JobRunner`.
    async fn run_transient(
        &self,
        spec: &ProcessSpec,
        run_id: JobRunId,
    ) -> Result<TransientOutcome, SpawnError>;
}
