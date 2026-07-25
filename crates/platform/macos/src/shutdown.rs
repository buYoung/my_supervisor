//! `UnixShutdown` — graceful (signal → grace → SIGKILL) shutdown of Direct-mode
//! children by signalling their process group.

use std::time::Duration;

use async_trait::async_trait;

use my_supervisor_core::domain::{ChildHandle, ShutdownPolicy, ShutdownSignal};
use my_supervisor_core::ports::shutdown::{ShutdownSignaler, SignalError};

use crate::signals::{
    process_group_exists, signal_checked, signal_known_owned_group, SIGINT, SIGKILL, SIGTERM,
};

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const FORCE_KILL_WAIT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, Default)]
pub struct UnixShutdown;

impl UnixShutdown {
    pub fn new() -> Self {
        UnixShutdown
    }
}

/// End a dedicated, previously verified process group. The leader may exit
/// before its descendants, so only `kill(-pgid, 0) == ESRCH` is terminal
/// success. `EPERM` and identity mismatches deliberately remain errors.
pub(crate) async fn terminate_owned_process_group(
    target: &ChildHandle,
    initial_signal: i32,
    grace_period: Duration,
    mut owned_leader: Option<&mut tokio::process::Child>,
) -> Result<(), SignalError> {
    match signal_checked(target, initial_signal) {
        Ok(()) => {}
        Err(SignalError::AlreadyExited) => {
            if !process_group_exists(owned_group_id(target)?)? {
                return Ok(());
            }
            signal_known_owned_group(target, initial_signal)?;
        }
        Err(error) => return Err(error),
    }

    if wait_for_group_exit(target, grace_period, &mut owned_leader).await? {
        return Ok(());
    }

    // Do not re-check the leader here: it can have exited after the first
    // checked signal while a TERM-ignoring grandchild remains in the same
    // dedicated group. The still-existing group ID pins that group identity.
    match signal_known_owned_group(target, SIGKILL) {
        Ok(()) | Err(SignalError::AlreadyExited) => {}
        Err(error) => return Err(error),
    }
    if wait_for_group_exit(target, FORCE_KILL_WAIT, &mut owned_leader).await? {
        return Ok(());
    }
    Err(SignalError::GroupExitTimeout(owned_group_id(target)?))
}

fn owned_group_id(target: &ChildHandle) -> Result<u32, SignalError> {
    match (target.pgid, target.generation.as_deref()) {
        (Some(pgid), Some(generation))
            if pgid != 0 && pgid == target.pid && !generation.is_empty() => Ok(pgid),
        _ => Err(SignalError::IdentityMismatch),
    }
}

async fn wait_for_group_exit(
    target: &ChildHandle,
    timeout: Duration,
    owned_leader: &mut Option<&mut tokio::process::Child>,
) -> Result<bool, SignalError> {
    let pgid = owned_group_id(target)?;
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match process_group_exists(pgid) {
            Ok(false) => return Ok(true),
            Ok(true) => {}
            Err(SignalError::PermissionDenied) => {
                // On macOS a group containing only our unreaped zombie leader
                // can report EPERM to `kill(-pgid, 0)`. Reap the Child we own,
                // then probe again; EPERM itself is never considered success.
                if let Some(child) = owned_leader.as_deref_mut() {
                    child
                        .try_wait()
                        .map_err(|error| SignalError::IoFailure(error.to_string()))?;
                }
            }
            Err(error) => return Err(error),
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(false);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
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
        terminate_owned_process_group(target, sig, cfg.grace_period, None).await
    }

    async fn force_kill(&self, target: &ChildHandle) -> Result<(), SignalError> {
        terminate_owned_process_group(target, SIGKILL, Duration::ZERO, None).await
    }
}
