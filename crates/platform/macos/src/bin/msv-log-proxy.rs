//! Detached-process log owner.  It remains the process-group leader and
//! serializes both target streams into one durable, sequence-numbered journal.

use std::path::PathBuf;
use std::process::Stdio;
use std::{io::{self, Read, Write}, os::fd::{AsRawFd, FromRawFd}};
use std::os::unix::ffi::OsStrExt;

use chrono::Utc;
use my_supervisor_platform_macos::process_identity::snapshot;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
    System,
}

impl Stream {
    fn name(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::System => "system",
        }
    }
}

fn usage() -> ! {
    eprintln!("usage: msv-log-proxy --journal <path> --control-fd <fd> -- <command> [args...]");
    std::process::exit(2);
}

struct ReaperControl {
    streams: Vec<std::os::unix::net::UnixStream>,
}

impl ReaperControl {
    fn connect(descriptors: Vec<i32>) -> Result<Self, String> {
        if descriptors.len() < 2 {
            return Err("detached target requires two cleanup owners".into());
        }
        // SAFETY: every descriptor is the proxy side of a socket pair created
        // by `spawn_detached_child`, and the proxy owns each one after exec.
        let mut streams = descriptors
            .into_iter()
            .map(|descriptor| unsafe { std::os::unix::net::UnixStream::from_raw_fd(descriptor) })
            .collect::<Vec<_>>();
        for stream in &mut streams {
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .map_err(|error| error.to_string())?;
            set_close_on_exec(stream).map_err(|error| error.to_string())?;
        }
        let identity = snapshot(std::process::id()).map_err(|error| error.to_string())?;
        if identity.pgid != identity.pid || identity.is_zombie() {
            return Err("proxy does not own a live dedicated process group".into());
        }
        for stream in &mut streams {
            write_control_line(
                stream,
                &format!("READY {} {} {}", identity.pid, identity.pgid, identity.generation),
            )
            .map_err(|error| error.to_string())?;
        }
        for stream in &mut streams {
            match read_control_line(stream).map_err(|error| error.to_string())? {
                Some(reply) if reply == "READY" => {}
                Some(reply) => return Err(format!("unexpected detached reaper setup response: {reply}")),
                None => return Err("detached reaper closed its setup channel".into()),
            }
        }
        Ok(Self { streams })
    }

    fn takeover(&mut self) -> Result<(), String> {
        for stream in &mut self.streams {
            write_control_line(stream, "TAKEOVER").map_err(|error| error.to_string())?;
        }
        self.expect_all("TAKEN_OVER", "takeover")
    }

    fn complete(&mut self) -> Result<(), String> {
        for stream in &mut self.streams {
            write_control_line(stream, "COMPLETE").map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn arm_anchor(&mut self, anchor_pid: u32, pgid: u32, generation: &str, journal: &std::path::Path) -> Result<(), String> {
        let journal = journal
            .as_os_str()
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        for stream in &mut self.streams {
            write_control_line(stream, &format!("ANCHOR {anchor_pid} {pgid} {generation} {journal}"))
                .map_err(|error| error.to_string())?;
        }
        self.expect_all("ARMED", "anchor")
    }

    fn start_target(&mut self) -> Result<(), String> {
        for stream in &mut self.streams {
            write_control_line(stream, "START").map_err(|error| error.to_string())?;
        }
        self.expect_all("STARTED", "target start")
    }

    fn expect_all(&mut self, expected: &str, operation: &str) -> Result<(), String> {
        for stream in &mut self.streams {
            match read_control_line(stream).map_err(|error| error.to_string())? {
                Some(reply) if reply == expected => {}
                Some(reply) => return Err(format!("unexpected detached reaper {operation} response: {reply}")),
                None => return Err(format!("detached reaper closed before {operation} confirmation")),
            }
        }
        Ok(())
    }
}

fn set_close_on_exec(stream: &std::os::unix::net::UnixStream) -> io::Result<()> {
    let descriptor = stream.as_raw_fd();
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn read_control_line(stream: &mut std::os::unix::net::UnixStream) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) if bytes.is_empty() => return Ok(None),
            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "partial reaper control response")),
            Ok(_) if byte[0] == b'\n' => return String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid reaper control response")),
            Ok(_) => bytes.push(byte[0]),
            Err(error) => return Err(error),
        }
    }
}

fn write_control_line(stream: &mut std::os::unix::net::UnixStream, message: &str) -> io::Result<()> {
    stream.write_all(message.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

async fn pump<R>(reader: R, stream: Stream, tx: mpsc::Sender<(Stream, String)>)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if tx.send((stream, line)).await.is_err() {
            break;
        }
    }
}

async fn append(
    file: &mut tokio::fs::File,
    sequence: u64,
    stream: Stream,
    line: String,
    remaining_successful_appends: &mut Option<u64>,
) -> std::io::Result<()> {
    if let Some(remaining) = remaining_successful_appends {
        if *remaining == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "injected journal append failure",
            ));
        }
        *remaining -= 1;
    }
    let encoded = serde_json::json!({
        "sequence": sequence,
        "timestamp": Utc::now(),
        "stream": stream.name(),
        "line": line,
    })
    .to_string();
    file.write_all(encoded.as_bytes()).await?;
    file.write_all(b"\n").await?;
    file.flush().await?;
    file.sync_data().await
}

async fn spawn_anchor() -> Result<tokio::process::Child, String> {
    let proxy_path = std::env::current_exe().map_err(|error| error.to_string())?;
    tokio::process::Command::new(proxy_path)
        .arg("--group-anchor")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())
}

async fn stop_anchor(anchor: &mut tokio::process::Child) {
    let _ = anchor.start_kill();
    let _ = anchor.wait().await;
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    if first.as_deref() == Some(std::ffi::OsStr::new("--group-anchor")) {
        if args.next().is_some() {
            usage();
        }
        std::future::pending::<()>().await;
        return;
    }
    let mut fail_after_appends = None;
    let journal_marker = if first.as_deref() == Some(std::ffi::OsStr::new("--test-fail-after-appends")) {
        #[cfg(debug_assertions)]
        {
            fail_after_appends = args
                .next()
                .and_then(|value| value.to_string_lossy().parse::<u64>().ok());
            if fail_after_appends.is_none() {
                usage();
            }
            args.next()
        }
        #[cfg(not(debug_assertions))]
        usage()
    } else {
        first
    };
    if journal_marker.as_deref() != Some(std::ffi::OsStr::new("--journal")) {
        usage();
    }
    let journal = args.next().map(PathBuf::from).unwrap_or_else(|| usage());
    let remaining = args.collect::<Vec<_>>();
    let mut position = 0;
    let mut control_descriptors = Vec::new();
    while remaining.get(position).map(std::ffi::OsString::as_os_str) == Some(std::ffi::OsStr::new("--control-fd")) {
        position += 1;
        let descriptor = remaining
            .get(position)
            .and_then(|value| value.to_string_lossy().parse::<i32>().ok())
            .filter(|value| *value >= 0)
            .unwrap_or_else(|| usage());
        control_descriptors.push(descriptor);
        position += 1;
    }
    if remaining.get(position).map(std::ffi::OsString::as_os_str) != Some(std::ffi::OsStr::new("--")) {
        usage();
    }
    position += 1;
    let command = remaining.get(position).cloned().unwrap_or_else(|| usage());
    position += 1;
    let command_args = remaining[position..].to_vec();
    let mut reaper = match ReaperControl::connect(control_descriptors) {
        Ok(reaper) => reaper,
        Err(error) => {
            eprintln!("initializing detached group reaper failed: {error}");
            std::process::exit(1);
        }
    };
    if let Some(parent) = journal.parent() {
        if let Err(error) = tokio::fs::create_dir_all(parent).await {
            eprintln!("creating journal directory failed: {error}");
            std::process::exit(1);
        }
    }
    let mut journal_file = match tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&journal)
        .await
    {
        Ok(file) => file,
        Err(error) => {
            eprintln!("opening journal failed: {error}");
            std::process::exit(1);
        }
    };
    let existing = tokio::fs::read_to_string(&journal).await.unwrap_or_default();
    let mut sequence = existing
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|value| value.get("sequence").and_then(|value| value.as_u64()))
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let mut remaining_successful_appends = fail_after_appends;

    let mut anchor = match spawn_anchor().await {
        Ok(anchor) => anchor,
        Err(error) => {
            eprintln!("starting detached group anchor failed: {error}");
            let _ = reaper.complete();
            std::process::exit(1);
        }
    };
    let anchor_identity = match snapshot(anchor.id().unwrap_or(0)) {
        Ok(identity) if identity.pgid == std::process::id() && !identity.is_zombie() => identity,
        Ok(_) | Err(_) => {
            eprintln!("detached group anchor did not expose a matching live identity");
            stop_anchor(&mut anchor).await;
            let _ = reaper.complete();
            std::process::exit(1);
        }
    };
    if let Err(error) = reaper.arm_anchor(anchor_identity.pid, anchor_identity.pgid, &anchor_identity.generation, &journal) {
        eprintln!("arming detached group cleanup owners failed: {error}");
        stop_anchor(&mut anchor).await;
        std::process::exit(1);
    }
    if let Err(error) = reaper.start_target() {
        eprintln!("confirming detached cleanup owners before target start failed: {error}");
        stop_anchor(&mut anchor).await;
        std::process::exit(1);
    }

    let mut target = match tokio::process::Command::new(command)
        .args(command_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            let _ = append(&mut journal_file, sequence, Stream::System, format!("target spawn failed: {error}"), &mut remaining_successful_appends).await;
            let _ = reaper.complete();
            stop_anchor(&mut anchor).await;
            std::process::exit(127);
        }
    };
    let stdout = target.stdout.take();
    let stderr = target.stderr.take();
    let (tx, mut rx) = mpsc::channel(256);
    let stdout_pump = stdout.map(|stdout| tokio::spawn(pump(stdout, Stream::Stdout, tx.clone())));
    let stderr_pump = stderr.map(|stderr| tokio::spawn(pump(stderr, Stream::Stderr, tx.clone())));
    drop(tx);
    let mut target_status = None;
    let mut lines_closed = false;
    while target_status.is_none() || !lines_closed {
        tokio::select! {
            line = rx.recv(), if !lines_closed => {
                match line {
                    Some((stream, line)) => {
                        if let Err(error) = append(&mut journal_file, sequence, stream, line, &mut remaining_successful_appends).await {
                            eprintln!("writing detached journal failed: {error}");
                            drop(journal_file);
                            let _ = tokio::fs::remove_file(&journal).await;
                            if let Err(takeover_error) = reaper.takeover() {
                                eprintln!("detached group reaper takeover failed: {takeover_error}");
                            }
                            // The helper now owns (or explicitly failed to own)
                            // the whole target group.  Do not reduce this to a
                            // direct-child kill, which would leak grandchildren.
                            std::process::exit(1);
                        }
                        sequence = sequence.saturating_add(1);
                    }
                    None => lines_closed = true,
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(20)), if target_status.is_none() => {
                target_status = match target.try_wait() {
                    Ok(Some(status)) => Some(Ok(status)),
                    Ok(None) => None,
                    Err(error) => Some(Err(error)),
                };
            }
        }
    }
    let status = target_status.unwrap_or_else(|| Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "target wait did not complete",
    )));
    if let Some(pump) = stdout_pump { let _ = pump.await; }
    if let Some(pump) = stderr_pump { let _ = pump.await; }
    let (exit_code, terminal) = match status {
        Ok(status) => (status.code().unwrap_or(1), format!("target exited: {status}")),
        Err(error) => (1, format!("target wait failed: {error}")),
    };
    if let Err(error) = append(&mut journal_file, sequence, Stream::System, terminal, &mut remaining_successful_appends).await {
        eprintln!("writing detached terminal journal failed: {error}");
        drop(journal_file);
        let _ = tokio::fs::remove_file(&journal).await;
        if let Err(takeover_error) = reaper.takeover() {
            eprintln!("detached group reaper takeover failed: {takeover_error}");
        }
        std::process::exit(1);
    }
    if let Err(error) = reaper.complete() {
        eprintln!("completing detached group reaper failed: {error}");
        stop_anchor(&mut anchor).await;
        std::process::exit(1);
    }
    stop_anchor(&mut anchor).await;
    std::process::exit(exit_code);
}
