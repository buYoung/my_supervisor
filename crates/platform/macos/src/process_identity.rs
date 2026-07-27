//! Native macOS process identity snapshots.
//!
//! `proc_pidinfo(PROC_PIDTBSDINFO)` provides PID, process group and creation
//! time from one kernel snapshot.  Keeping this small FFI here prevents the
//! rest of the lifecycle code from treating a human-readable `ps` timestamp as
//! an identity proof.

use my_supervisor_core::ports::SignalError;

const PROC_PIDTBSDINFO: i32 = 3;
const PROC_PIDTBSDINFO_SIZE: i32 = std::mem::size_of::<ProcBsdInfo>() as i32;
const SZOMB: u32 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub pgid: u32,
    pub status: u32,
    pub generation: String,
}

impl ProcessIdentity {
    pub fn is_zombie(&self) -> bool {
        self.status == SZOMB
    }
}

// This layout mirrors `struct proc_bsdinfo` in macOS `proc_info.h`.  Only the
// fields used below are interpreted.  Keep the reserved field and the name
// buffers byte-for-byte identical to the SDK declaration: compensating for a
// missing field with larger arrays would accidentally preserve some offsets
// while no longer describing the C ABI.
#[repr(C)]
struct ProcBsdInfo {
    pbi_flags: u32,
    pbi_status: u32,
    pbi_xstatus: u32,
    pbi_pid: u32,
    pbi_ppid: u32,
    pbi_uid: u32,
    pbi_gid: u32,
    pbi_ruid: u32,
    pbi_rgid: u32,
    pbi_svuid: u32,
    pbi_svgid: u32,
    pbi_rfu_1: u32,
    pbi_comm: [libc::c_char; 16],
    pbi_name: [libc::c_char; 32],
    pbi_nfiles: u32,
    pbi_pgid: u32,
    pbi_pjobc: u32,
    e_tdev: u32,
    e_tpgid: u32,
    pbi_nice: i32,
    pbi_start_tvsec: u64,
    pbi_start_tvusec: u64,
}

// `struct proc_bsdinfo` from macOS `sys/proc_info.h` is 136 bytes on the
// supported 64-bit macOS ABI.  Assert both the full layout and every field we
// read so SDK changes cannot silently turn this identity snapshot into an
// offset guess.
const _: [(); 136] = [(); std::mem::size_of::<ProcBsdInfo>()];
const _: [(); 4] = [(); std::mem::offset_of!(ProcBsdInfo, pbi_status)];
const _: [(); 12] = [(); std::mem::offset_of!(ProcBsdInfo, pbi_pid)];
const _: [(); 100] = [(); std::mem::offset_of!(ProcBsdInfo, pbi_pgid)];
const _: [(); 120] = [(); std::mem::offset_of!(ProcBsdInfo, pbi_start_tvsec)];
const _: [(); 128] = [(); std::mem::offset_of!(ProcBsdInfo, pbi_start_tvusec)];

#[cfg(target_os = "macos")]
#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pidinfo(
        pid: libc::c_int,
        flavor: libc::c_int,
        arg: u64,
        buffer: *mut libc::c_void,
        buffer_size: libc::c_int,
    ) -> libc::c_int;
}

pub fn snapshot(pid: u32) -> Result<ProcessIdentity, SignalError> {
    if pid == 0 {
        return Err(SignalError::AlreadyExited);
    }
    #[cfg(target_os = "macos")]
    {
        let mut info = std::mem::MaybeUninit::<ProcBsdInfo>::zeroed();
        // SAFETY: `info` is a properly sized writable `proc_bsdinfo` buffer;
        // the function only fills that buffer for the requested PID.
        let result = unsafe {
            proc_pidinfo(
                pid as libc::c_int,
                PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                PROC_PIDTBSDINFO_SIZE,
            )
        };
        if result == PROC_PIDTBSDINFO_SIZE {
            // SAFETY: a full-size successful call initialized every field.
            let info = unsafe { info.assume_init() };
            if info.pbi_pid != pid {
                return Err(SignalError::IdentityMismatch);
            }
            return Ok(ProcessIdentity {
                pid: info.pbi_pid,
                pgid: info.pbi_pgid,
                status: info.pbi_status,
                generation: format!(
                    "macos-libproc:{}:{}",
                    info.pbi_start_tvsec, info.pbi_start_tvusec
                ),
            });
        }
        Err(last_error())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        Err(SignalError::IoFailure(
            "native macOS process identity is unavailable on this target".into(),
        ))
    }
}

/// Verify the dedicated process-group leader immediately before a detached
/// cleanup helper sends a group signal.
pub fn verify_group_leader(
    pid: u32,
    pgid: u32,
    expected_generation: &str,
) -> Result<(), SignalError> {
    let identity = snapshot(pid)?;
    if identity.is_zombie()
        || identity.pid != pid
        || identity.pgid != pgid
        || identity.generation != expected_generation
    {
        return Err(SignalError::IdentityMismatch);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn last_error() -> SignalError {
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => SignalError::AlreadyExited,
        Some(libc::EPERM) => SignalError::PermissionDenied,
        _ => SignalError::IoFailure(std::io::Error::last_os_error().to_string()),
    }
}
