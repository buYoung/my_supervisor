//! Thin libc signal helpers.  A process group is signalable only while its
//! leader, group id, and creation token still match the recorded handle.

use my_supervisor_core::domain::ChildHandle;
use my_supervisor_core::ports::SignalError;

use crate::process_identity::{snapshot, ProcessIdentity};

pub fn process_identity(pid: u32) -> Result<ProcessIdentity, SignalError> { snapshot(pid) }

pub fn matches_handle(handle: &ChildHandle) -> bool {
    let Some(pgid) = handle.pgid else {
        return false;
    };
    let Some(expected_generation) = handle.generation.as_deref() else {
        return false;
    };
    process_identity(handle.pid).is_ok_and(|actual| {
        pgid != 0
            && pgid == handle.pid
            && actual.pid == handle.pid
            && actual.pgid == pgid
            && !actual.is_zombie()
            && actual.generation == expected_generation
    })
}

/// Signal a verified dedicated process group.  `ESRCH` is idempotent; every
/// other failed verification is returned to the caller instead of risking a
/// signal to a recycled PID.
pub fn signal_checked(handle: &ChildHandle, signal: i32) -> Result<(), SignalError> {
    signal_checked_with(handle, signal, process_identity, |pgid, signal| {
        // SAFETY: the group identity was validated immediately above and
        // negative pgid targets only the session created by this supervisor.
        if unsafe { libc::kill(-(pgid as i32), signal) } == 0 {
            return Ok(());
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => Err(SignalError::AlreadyExited),
            Some(libc::EPERM) => Err(SignalError::PermissionDenied),
            _ => Err(SignalError::IoFailure(std::io::Error::last_os_error().to_string())),
        }
    })
}

/// Signal the group already proven to be dedicated to `handle`. This is only
/// valid after a checked signal has established ownership, or when the leader
/// has become a same-group zombie/exited process while the group still exists.
/// A live mismatched leader must always be rejected by `signal_checked` before
/// this fallback is used.
pub(crate) fn signal_known_owned_group(
    handle: &ChildHandle,
    signal: i32,
) -> Result<(), SignalError> {
    let pgid = dedicated_group_id(handle)?;
    signal_group(pgid, signal)
}

fn dedicated_group_id(handle: &ChildHandle) -> Result<u32, SignalError> {
    let Some(pgid) = handle.pgid else {
        return Err(SignalError::IdentityMismatch);
    };
    if pgid == 0 || pgid != handle.pid || handle.generation.as_deref().is_none_or(str::is_empty)
    {
        return Err(SignalError::IdentityMismatch);
    }
    Ok(pgid)
}

fn signal_group(pgid: u32, signal: i32) -> Result<(), SignalError> {
    signal_group_with(pgid, signal, |group, signal| {
        // SAFETY: callers only pass a dedicated group previously verified for
        // this supervisor. Negative group IDs cannot target a single recycled
        // leader PID while the original group remains alive.
        if unsafe { libc::kill(-(group as i32), signal) } == 0 {
            return Ok(());
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => Err(SignalError::AlreadyExited),
            Some(libc::EPERM) => Err(SignalError::PermissionDenied),
            _ => Err(SignalError::IoFailure(std::io::Error::last_os_error().to_string())),
        }
    })
}

fn signal_group_with(
    pgid: u32,
    signal: i32,
    send_signal: impl FnOnce(u32, i32) -> Result<(), SignalError>,
) -> Result<(), SignalError> {
    if pgid == 0 {
        return Err(SignalError::IdentityMismatch);
    }
    send_signal(pgid, signal)
}

fn signal_checked_with(
    handle: &ChildHandle,
    signal: i32,
    snapshot: impl Fn(u32) -> Result<ProcessIdentity, SignalError>,
    send_signal: impl FnOnce(u32, i32) -> Result<(), SignalError>,
) -> Result<(), SignalError> {
    let before = snapshot(handle.pid)?;
    if before.is_zombie() {
        return Err(SignalError::AlreadyExited);
    }
    if !matches_identity(handle, &before) {
        return Err(SignalError::IdentityMismatch);
    }

    match send_signal(before.pgid, signal) {
        Ok(()) | Err(SignalError::AlreadyExited) => {}
        Err(error) => return Err(error),
    }

    // A successfully signalled child can be observed as a zombie before its
    // owner reaches Child::wait.  A zombie still reserves its PID, so its
    // owned Child is the authority that reaps it; only a changed *live*
    // identity can be a recycled PID that makes another signal unsafe.
    match snapshot(handle.pid) {
        Ok(after) if after.is_zombie() || matches_identity(handle, &after) => Ok(()),
        Ok(_) => Err(SignalError::IdentityMismatch),
        // The raw group signal already succeeded against the just-verified
        // native identity. macOS can reject a follow-up `proc_pidinfo` while
        // that leader is becoming a zombie; group absence is still verified by
        // the caller, so this probe race must not turn a delivered SIGKILL into
        // a false permission failure.
        Err(SignalError::AlreadyExited | SignalError::PermissionDenied) => Ok(()),
        Err(error) => Err(error),
    }
}

fn matches_identity(handle: &ChildHandle, actual: &ProcessIdentity) -> bool {
    let Some(pgid) = handle.pgid else {
        return false;
    };
    let Some(expected_generation) = handle.generation.as_deref() else {
        return false;
    };
    pgid != 0
        && pgid == handle.pid
        && actual.pid == handle.pid
        && actual.pgid == pgid
        && actual.generation == expected_generation
}

/// Read process existence without reducing `EPERM` to a false "exited"
/// result.  Mutation paths must treat a permission denial as unverified.
pub fn process_exists(pid: u32) -> Result<bool, SignalError> {
    if pid == 0 {
        return Ok(false);
    }
    match process_identity(pid) {
        Ok(identity) => Ok(!identity.is_zombie()),
        Err(SignalError::AlreadyExited) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Equivalent checked existence probe for the dedicated process group.
pub fn process_group_exists(pgid: u32) -> Result<bool, SignalError> {
    if pgid == 0 {
        return Ok(false);
    }
    // SAFETY: signal 0 only queries the dedicated group recorded in a handle.
    if unsafe { libc::kill(-(pgid as i32), 0) } == 0 {
        return Ok(true);
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Err(SignalError::PermissionDenied),
        _ => Err(SignalError::IoFailure(std::io::Error::last_os_error().to_string())),
    }
}

pub const SIGTERM: i32 = libc::SIGTERM;
pub const SIGINT: i32 = libc::SIGINT;
pub const SIGKILL: i32 = libc::SIGKILL;

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use chrono::Utc;
    use my_supervisor_core::domain::ChildHandle;
    use my_supervisor_core::ports::SignalError;
    use uuid::Uuid;

    use super::{signal_checked_with, signal_group_with, ProcessIdentity};

    fn handle() -> ChildHandle {
        ChildHandle {
            process_id: Uuid::new_v4(),
            pid: 42,
            pgid: Some(42),
            generation: Some("macos-libproc:10:20".into()),
            started_at: Utc::now(),
        }
    }

    fn identity(status: u32, generation: &str) -> ProcessIdentity {
        ProcessIdentity {
            pid: 42,
            pgid: 42,
            status,
            generation: generation.into(),
        }
    }

    #[test]
    fn post_signal_same_generation_zombie_is_left_for_child_wait() {
        let snapshots = RefCell::new(VecDeque::from([
            Ok(identity(1, "macos-libproc:10:20")),
            Ok(identity(5, "macos-libproc:10:20")),
        ]));

        let result = signal_checked_with(
            &handle(),
            libc::SIGTERM,
            |_| snapshots.borrow_mut().pop_front().unwrap(),
            |_, _| Ok(()),
        );

        assert!(matches!(result, Ok(())));
    }

    #[test]
    fn post_signal_live_generation_mismatch_is_rejected() {
        let snapshots = RefCell::new(VecDeque::from([
            Ok(identity(1, "macos-libproc:10:20")),
            Ok(identity(1, "macos-libproc:10:21")),
        ]));

        let result = signal_checked_with(
            &handle(),
            libc::SIGTERM,
            |_| snapshots.borrow_mut().pop_front().unwrap(),
            |_, _| Ok(()),
        );

        assert!(matches!(result, Err(SignalError::IdentityMismatch)));
    }

    #[test]
    fn post_signal_permission_probe_does_not_override_a_delivered_signal() {
        let snapshots = RefCell::new(VecDeque::from([
            Ok(identity(1, "macos-libproc:10:20")),
            Err(SignalError::PermissionDenied),
        ]));

        let result = signal_checked_with(
            &handle(),
            libc::SIGKILL,
            |_| snapshots.borrow_mut().pop_front().unwrap(),
            |_, _| Ok(()),
        );

        assert!(matches!(result, Ok(())));
    }

    #[test]
    fn permission_denied_and_missing_group_remain_distinct() {
        let permission_denied = signal_checked_with(
            &handle(),
            libc::SIGTERM,
            |_| Ok(identity(1, "macos-libproc:10:20")),
            |_, _| Err(SignalError::PermissionDenied),
        );
        let missing_group = signal_checked_with(
            &handle(),
            libc::SIGTERM,
            |_| Ok(identity(1, "macos-libproc:10:20")),
            |_, _| Err(SignalError::AlreadyExited),
        );

        assert!(matches!(permission_denied, Err(SignalError::PermissionDenied)));
        assert!(matches!(missing_group, Ok(())));
    }

    #[test]
    fn known_group_signal_keeps_permission_and_missing_group_distinct() {
        let permission_denied = signal_group_with(42, libc::SIGKILL, |_, _| {
            Err(SignalError::PermissionDenied)
        });
        let missing_group = signal_group_with(42, libc::SIGKILL, |_, _| {
            Err(SignalError::AlreadyExited)
        });

        assert!(matches!(permission_denied, Err(SignalError::PermissionDenied)));
        assert!(matches!(missing_group, Err(SignalError::AlreadyExited)));
    }
}
