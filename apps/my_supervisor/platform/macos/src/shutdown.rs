//! `UnixShutdown` — graceful (signal → grace → SIGKILL) shutdown of Direct-mode
//! children by signalling their process group.

use std::time::Duration;

use async_trait::async_trait;

use my_supervisor_core::domain::{ChildHandle, ShutdownPolicy, ShutdownSignal};
use my_supervisor_core::ports::shutdown::{ShutdownSignaler, SignalError};

use crate::signals::{is_alive, signal_group, SIGINT, SIGKILL, SIGTERM};

const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, Default)]
pub struct UnixShutdown;

impl UnixShutdown {
    pub fn new() -> Self {
        UnixShutdown
    }
}

#[async_trait]
impl ShutdownSignaler for UnixShutdown {
    async fn request_graceful(
        &self,
        target: &ChildHandle,
        cfg: &ShutdownPolicy,
    ) -> Result<(), SignalError> {
        let sig = match cfg.signal {
            ShutdownSignal::Term => SIGTERM,
            ShutdownSignal::Int => SIGINT,
            ShutdownSignal::Kill => SIGKILL,
        };
        signal_group(target.pid, sig);

        // Wait out the grace period, polling for early exit.
        let deadline = tokio::time::Instant::now() + cfg.grace_period;
        loop {
            if !is_alive(target.pid) {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }

        if is_alive(target.pid) {
            signal_group(target.pid, SIGKILL);
        }
        Ok(())
    }

    async fn force_kill(&self, target: &ChildHandle) -> Result<(), SignalError> {
        signal_group(target.pid, SIGKILL);
        Ok(())
    }
}
