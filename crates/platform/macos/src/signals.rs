//! Thin libc signal helpers. Children are session leaders (`setsid`), so `pgid`
//! equals `pid` and `kill(-pid, …)` targets the whole tree.

/// Signal an entire process group (negative pid).
pub fn signal_group(pid: u32, sig: i32) -> bool {
    if pid == 0 {
        return false;
    }
    // SAFETY: kill with a valid pid and signal number; failures are reported, not UB.
    unsafe { libc::kill(-(pid as i32), sig) == 0 }
}

/// True if the process is still alive (`kill(pid, 0)` succeeds).
pub fn is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // SAFETY: signal 0 only checks for existence/permission.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// True when the process still leads the dedicated group created by `setsid`.
/// This reduces the chance of adopting an unrelated process after PID reuse.
pub fn is_process_group_leader(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // SAFETY: getpgid only reads kernel process metadata for the supplied PID.
    unsafe { libc::getpgid(pid as i32) == pid as i32 }
}

pub const SIGTERM: i32 = libc::SIGTERM;
pub const SIGINT: i32 = libc::SIGINT;
pub const SIGKILL: i32 = libc::SIGKILL;
