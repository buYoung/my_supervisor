//! Detached-process group cleanup owner.
//!
//! The log proxy leads the target group and therefore cannot safely signal the
//! group after a journal failure: doing so would terminate the only process
//! that can coordinate cleanup.  This helper lives in another session and
//! takes over that responsibility through a private inherited socket.

use std::io::{Read, Write};
use std::os::{fd::FromRawFd, unix::ffi::OsStringExt};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use my_supervisor_platform_macos::process_identity::{snapshot, verify_group_leader};
use my_supervisor_core::ports::SignalError;

const GRACE_PERIOD: Duration = Duration::from_secs(2);
const GROUP_EXIT_TIMEOUT: Duration = Duration::from_secs(5);

struct GroupIdentity {
    pid: u32,
    pgid: u32,
    generation: String,
    journal: Option<PathBuf>,
}

fn usage() -> ! {
    eprintln!("usage: msv-group-reaper --control-fd <fd>");
    std::process::exit(2);
}

fn read_line(stream: &mut std::os::unix::net::UnixStream) -> std::io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) if bytes.is_empty() => return Ok(None),
            Ok(0) => return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "partial control message")),
            Ok(_) if byte[0] == b'\n' => return String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "control message is not UTF-8")),
            Ok(_) => bytes.push(byte[0]),
            Err(error) => return Err(error),
        }
    }
}

fn write_line(stream: &mut std::os::unix::net::UnixStream, message: &str) -> std::io::Result<()> {
    stream.write_all(message.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

fn parse_identity(message: &str, expected_message: &str) -> Result<GroupIdentity, String> {
    let mut parts = message.splitn(5, ' ');
    if parts.next() != Some(expected_message) {
        return Err(format!("expected {expected_message} control message"));
    }
    let pid = parts
        .next()
        .ok_or("missing proxy PID")?
        .parse()
        .map_err(|_| "invalid proxy PID")?;
    let pgid = parts
        .next()
        .ok_or("missing target PGID")?
        .parse()
        .map_err(|_| "invalid target PGID")?;
    let generation = parts.next().ok_or("missing proxy generation")?.to_owned();
    if pid == 0 || pgid == 0 || generation.is_empty() || generation.contains(char::is_whitespace) {
        return Err("invalid target group identity".into());
    }
    let journal = match (expected_message, parts.next()) {
        ("ANCHOR", Some(encoded)) => Some(PathBuf::from(std::ffi::OsString::from_vec(decode_path(encoded)?))),
        ("ANCHOR", None) => return Err("missing detached journal path".into()),
        (_, None) => None,
        _ => return Err("unexpected detached journal path".into()),
    };
    Ok(GroupIdentity { pid, pgid, generation, journal })
}

fn decode_path(encoded: &str) -> Result<Vec<u8>, String> {
    if encoded.len() % 2 != 0 {
        return Err("invalid detached journal path encoding".into());
    }
    (0..encoded.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&encoded[offset..offset + 2], 16).map_err(|_| "invalid detached journal path encoding".into()))
        .collect()
}

fn verify_anchor(identity: &GroupIdentity) -> Result<(), SignalError> {
    let current = snapshot(identity.pid)?;
    if current.is_zombie()
        || current.pid != identity.pid
        || current.pgid != identity.pgid
        || current.generation != identity.generation
    {
        return Err(SignalError::IdentityMismatch);
    }
    Ok(())
}

fn signal_group(pgid: u32, signal: i32) -> Result<bool, SignalError> {
    if unsafe { libc::kill(-(pgid as i32), signal) } == 0 {
        return Ok(true);
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Err(SignalError::PermissionDenied),
        _ => Err(SignalError::IoFailure(std::io::Error::last_os_error().to_string())),
    }
}

fn wait_for_group_exit(pgid: u32) -> Result<(), SignalError> {
    let deadline = Instant::now() + GROUP_EXIT_TIMEOUT;
    loop {
        if !signal_group(pgid, 0)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(SignalError::IoFailure(format!("process group {pgid} remained after detached cleanup")));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn reap_group(identity: &GroupIdentity) -> Result<(), SignalError> {
    // This is the only point at which a negative PID signal is introduced.
    // A live leader snapshot binds it to the original detached group; mismatch
    // and permission failures remain explicit failures rather than success.
    verify_anchor(identity)?;
    if !signal_group(identity.pgid, libc::SIGTERM)? {
        return Ok(());
    }
    std::thread::sleep(GRACE_PERIOD);
    if signal_group(identity.pgid, 0)? {
        let _ = signal_group(identity.pgid, libc::SIGKILL)?;
    }
    wait_for_group_exit(identity.pgid)
}

fn remove_failed_journal(identity: &GroupIdentity) -> Result<(), SignalError> {
    let Some(journal) = &identity.journal else {
        return Ok(());
    };
    match std::fs::remove_file(journal) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SignalError::IoFailure(format!("removing failed detached journal {}: {error}", journal.display()))),
    }
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--control-fd")) {
        usage();
    }
    let descriptor = args
        .next()
        .and_then(|value| value.to_string_lossy().parse::<i32>().ok())
        .filter(|value| *value >= 0)
        .unwrap_or_else(|| usage());
    let mut withhold_takeover_ack = false;
    let mut crash_after_start = false;
    while let Some(argument) = args.next() {
        match argument.to_string_lossy().as_ref() {
            "--test-withhold-takeover-ack" => {
                #[cfg(debug_assertions)]
                { withhold_takeover_ack = true; }
                #[cfg(not(debug_assertions))]
                usage();
            }
            "--test-crash-after-start" => {
                #[cfg(debug_assertions)]
                { crash_after_start = true; }
                #[cfg(not(debug_assertions))]
                usage();
            }
            _ => usage(),
        }
    }
    // SAFETY: `descriptor` is an inherited Unix-domain socket supplied only by
    // `spawn_detached_child`; this process becomes its sole owner after exec.
    let mut control = unsafe { std::os::unix::net::UnixStream::from_raw_fd(descriptor) };

    let ready = match read_line(&mut control) {
        Ok(Some(message)) => message,
        Ok(None) => std::process::exit(1),
        Err(error) => {
            eprintln!("reading detached reaper setup failed: {error}");
            std::process::exit(1);
        }
    };
    let proxy_identity = match parse_identity(&ready, "READY").and_then(|identity| {
        verify_group_leader(identity.pid, identity.pgid, &identity.generation)
            .map(|_| identity)
            .map_err(|error| error.to_string())
    }) {
        Ok(identity) => identity,
        Err(error) => {
            eprintln!("verifying detached target group failed: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = write_line(&mut control, "READY") {
        eprintln!("acknowledging detached reaper setup failed: {error}");
        std::process::exit(1);
    }

    let anchor_identity = match read_line(&mut control) {
        Ok(Some(message)) => {
            let anchor_identity = match parse_identity(&message, "ANCHOR").and_then(|identity| {
                if identity.pgid != proxy_identity.pgid {
                    return Err("anchor process group differs from proxy group".into());
                }
                verify_anchor(&identity).map(|_| identity).map_err(|error| error.to_string())
            }) {
                Ok(identity) => identity,
                Err(error) => {
                    eprintln!("verifying detached group anchor failed: {error}");
                    std::process::exit(1);
                }
            };
            if let Err(error) = write_line(&mut control, "ARMED") {
                eprintln!("acknowledging detached anchor failed: {error}");
                std::process::exit(1);
            }
            match read_line(&mut control) {
                Ok(Some(message)) if message == "START" => {
                    if let Err(error) = write_line(&mut control, "STARTED") {
                        eprintln!("acknowledging detached target start failed: {error}");
                        std::process::exit(1);
                    }
                    if crash_after_start {
                        std::process::exit(1);
                    }
                    anchor_identity
                }
                Ok(Some(message)) => {
                    eprintln!("invalid detached reaper start message: {message}");
                    std::process::exit(1);
                }
                Ok(None) => std::process::exit(1),
                Err(error) => {
                    eprintln!("reading detached reaper start failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        Ok(None) => std::process::exit(1),
        Err(error) => {
            eprintln!("reading detached anchor setup failed: {error}");
            std::process::exit(1);
        }
    };

    match read_line(&mut control) {
        Ok(Some(message)) if message == "COMPLETE" => std::process::exit(0),
        Ok(Some(message)) if message == "TAKEOVER" => {
            if !withhold_takeover_ack {
                if let Err(error) = write_line(&mut control, "TAKEN_OVER") {
                    eprintln!("acknowledging detached cleanup takeover failed: {error}");
                    std::process::exit(1);
                }
            }
            // When the acknowledgement is intentionally withheld, the proxy
            // exits nonzero and closes the channel.  The already-verified
            // helper still owns cleanup instead of leaving the target group.
            if let Err(error) = remove_failed_journal(&anchor_identity) {
                eprintln!("removing failed detached journal failed: {error}");
                std::process::exit(1);
            }
            if let Err(error) = reap_group(&anchor_identity) {
                eprintln!("detached target group cleanup failed: {error}");
                std::process::exit(1);
            }
            std::process::exit(0);
        }
        Ok(None) => {
            // A proxy crash after READY is not a normal completion.  The helper
            // retains enough verified identity to recover the target group.
            if let Err(error) = remove_failed_journal(&anchor_identity) {
                eprintln!("removing failed detached journal after proxy loss failed: {error}");
                std::process::exit(1);
            }
            if let Err(error) = reap_group(&anchor_identity) {
                eprintln!("detached target group cleanup after proxy loss failed: {error}");
                std::process::exit(1);
            }
            std::process::exit(0);
        }
        Ok(Some(message)) => {
            eprintln!("invalid detached reaper control message: {message}");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("reading detached reaper control failed: {error}");
            std::process::exit(1);
        }
    }
}
