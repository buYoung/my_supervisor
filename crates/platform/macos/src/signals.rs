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

pub const SIGTERM: i32 = libc::SIGTERM;
pub const SIGINT: i32 = libc::SIGINT;
pub const SIGKILL: i32 = libc::SIGKILL;
