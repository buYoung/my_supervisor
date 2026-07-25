//! `ProcessServiceRegistrar` — per-process SystemRegistered control. Distinct
//! from the daemon's own `AutoStartService`. The macOS `LaunchdAgentProcess`
//! adapter (child 06) implements this; foundation ships a null impl so the
//! Direct-mode walking skeleton builds.

use async_trait::async_trait;

use crate::domain::{LogLine, ProcessSpec, ProcessState};
use crate::ports::error::RegistrarError;

#[async_trait]
pub trait ProcessServiceRegistrar: Send + Sync {
    /// Register a process with the OS service manager under `unit_name`,
    /// generating the unit from the full spec (command, args, env, cwd, restart).
    async fn register(&self, unit_name: &str, spec: &ProcessSpec) -> Result<(), RegistrarError>;
    /// Remove the OS unit (idempotent).
    async fn unregister(&self, unit_name: &str) -> Result<(), RegistrarError>;
    async fn start(&self, unit_name: &str) -> Result<(), RegistrarError>;
    async fn stop(&self, unit_name: &str) -> Result<(), RegistrarError>;
    async fn query_status(&self, unit_name: &str) -> Result<ProcessState, RegistrarError>;
    /// Return the running service's process ID, or `None` while stopped.
    async fn query_pid(&self, unit_name: &str) -> Result<Option<u32>, RegistrarError>;
    async fn tail_logs(
        &self,
        unit_name: &str,
        lines: usize,
    ) -> Result<Vec<LogLine>, RegistrarError>;
}
