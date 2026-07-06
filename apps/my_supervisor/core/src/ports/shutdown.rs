//! `ShutdownSignaler` — graceful → force kill of Direct-mode children.

use async_trait::async_trait;
use thiserror::Error;

use crate::domain::{ChildHandle, ShutdownPolicy};

#[derive(Debug, Error)]
pub enum SignalError {
    #[error("signal failed: {0}")]
    Failed(String),
}

#[async_trait]
pub trait ShutdownSignaler: Send + Sync {
    /// Request a graceful stop, escalating to force kill after the grace period.
    async fn request_graceful(
        &self,
        target: &ChildHandle,
        cfg: &ShutdownPolicy,
    ) -> Result<(), SignalError>;
    /// Force-kill immediately.
    async fn force_kill(&self, target: &ChildHandle) -> Result<(), SignalError>;
}
