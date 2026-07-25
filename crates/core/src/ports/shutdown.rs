//! `ShutdownSignaler` — graceful → force kill of Direct-mode children.

use async_trait::async_trait;
use thiserror::Error;

use crate::domain::{ChildHandle, ShutdownPolicy};

#[derive(Debug, Error)]
pub enum SignalError {
    #[error("process already exited")]
    AlreadyExited,
    #[error("process identity no longer matches the recorded handle")]
    IdentityMismatch,
    #[error("permission denied while signalling process")]
    PermissionDenied,
    #[error("owned process group {0} remained after force kill")]
    GroupExitTimeout(u32),
    #[error("signal failed: {0}")]
    IoFailure(String),
}

#[async_trait]
pub trait ShutdownSignaler: Send + Sync {
    /// Request a graceful stop, escalating to force kill after the grace period.
    ///
    /// A successful return means the owned process group is absent, not merely
    /// that its original leader PID exited. This keeps descendants in the
    /// dedicated group from escaping a Direct-process stop.
    async fn request_graceful(
        &self,
        target: &ChildHandle,
        cfg: &ShutdownPolicy,
    ) -> Result<(), SignalError>;
    /// Force-kill immediately and wait until the owned process group is absent.
    async fn force_kill(&self, target: &ChildHandle) -> Result<(), SignalError>;
}
