//! Bounded macOS primitives used by the runtime-guard owner in application.
//!
//! This module deliberately does not decide restart policy or retain process
//! generation state.  It only exposes native observations whose bounds and
//! cleanup semantics are safe for that later owner to consume.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant, SystemTime};

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::watch;

use my_supervisor_core::domain::{
    CheckKind, CheckPolicy, ChildHandle, ProcessResourceUsage, ProcessSpec, WatchPolicy,
};
use my_supervisor_core::ports::lifecycle::{
    CheckDiagnostics, CheckOutcome, CheckStatus, GuardError, OwnedGroupResourceUsage,
    WatchObservation, WatchRegistrationId, WatchRescanReason,
};

use crate::signals::{matches_handle, process_group_exists, process_identity, SIGTERM};
use crate::spawn::spawn_child;

const CHECK_OUTPUT_LIMIT_BYTES: usize = 64 * 1024;
const CHECK_CLEANUP_GRACE: Duration = Duration::from_secs(2);
const CHECK_DRAIN_TIMEOUT: Duration = Duration::from_secs(3);
const WATCH_CHANNEL_CAPACITY: usize = 256;
const WATCH_BATCH_PATH_LIMIT: usize = 1_024;
const WATCH_RESCAN_ENTRY_LIMIT: usize = 10_000;

#[derive(Default)]
pub(crate) struct GuardRegistry {
    watches: HashMap<WatchRegistrationId, WatchEntry>,
}

struct WatchEntry {
    // Keeping the watcher in the registry keeps its native FSEvents run-loop
    // alive. Dropping this field is the resource-release boundary for stop.
    _watcher: RecommendedWatcher,
    receiver: mpsc::Receiver<notify::Result<Event>>,
    channel_overflow: Arc<AtomicBool>,
    policy: WatchPolicy,
    pending_paths: BTreeSet<PathBuf>,
    pending_since: Option<Instant>,
    snapshot: Snapshot,
}

type Snapshot = BTreeMap<PathBuf, FileStamp>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStamp {
    is_dir: bool,
    len: u64,
    modified: Option<SystemTime>,
}

pub(crate) async fn sample_owned_group_resource_usage(
    handle: &ChildHandle,
) -> Result<OwnedGroupResourceUsage, GuardError> {
    if !matches_handle(handle) {
        return Err(GuardError::Failed(
            "process identity no longer matches the recorded handle".into(),
        ));
    }
    let pgid = handle
        .pgid
        .ok_or_else(|| GuardError::Failed("missing owned process group".into()))?;
    let output = tokio::process::Command::new("ps")
        .env("LC_ALL", "C")
        .args(["-axo", "pid=,pgid=,rss="])
        .output()
        .await
        .map_err(|error| GuardError::Failed(error.to_string()))?;
    if !output.status.success() {
        return Ok(OwnedGroupResourceUsage::Unknown {
            reason: format!("cannot enumerate process groups: {}", output.status),
        });
    }

    let mut memory_bytes = 0_u64;
    let mut found_live_member = false;
    for line in String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let mut fields = line.split_whitespace();
        let Some(pid) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
            return Ok(OwnedGroupResourceUsage::Unknown {
                reason: "unparseable process enumeration row".into(),
            });
        };
        let Some(member_pgid) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
            return Ok(OwnedGroupResourceUsage::Unknown {
                reason: "missing process-group value".into(),
            });
        };
        let Some(rss_kilobytes) = fields.next().and_then(|value| value.parse::<u64>().ok()) else {
            return Ok(OwnedGroupResourceUsage::Unknown {
                reason: "missing resident-memory value".into(),
            });
        };
        if member_pgid != pgid {
            continue;
        }
        // `ps` supplies RSS, but native identity confirms that the member has
        // not moved, exited, or been recycled during the enumeration.
        match process_identity(pid) {
            Ok(identity) if identity.pgid == pgid && !identity.is_zombie() => {
                found_live_member = true;
                memory_bytes = memory_bytes.saturating_add(rss_kilobytes.saturating_mul(1024));
            }
            Ok(_) => {
                return Ok(OwnedGroupResourceUsage::Unknown {
                    reason: format!("process {pid} changed while sampling group"),
                })
            }
            Err(error) => {
                return Ok(OwnedGroupResourceUsage::Unknown {
                    reason: format!("cannot verify process {pid}: {error}"),
                })
            }
        }
    }
    if !found_live_member {
        return Ok(OwnedGroupResourceUsage::Unknown {
            reason: "owned process group has no verifiable live members".into(),
        });
    }
    if !matches_handle(handle) {
        return Err(GuardError::Failed(
            "process identity changed while collecting group resource usage".into(),
        ));
    }
    Ok(OwnedGroupResourceUsage::Sample(ProcessResourceUsage {
        cpu_percent: 0.0,
        memory_bytes,
    }))
}

pub(crate) async fn run_check(
    policy: &CheckPolicy,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<CheckOutcome, GuardError> {
    match &policy.kind {
        CheckKind::Exec { command, args } => {
            run_exec_check(command, args, policy.timeout, cancellation).await
        }
        CheckKind::Tcp { host, port } => {
            run_tcp_check(host, *port, policy.timeout, cancellation).await
        }
        CheckKind::Http {
            url,
            expected_status,
        } => run_http_check(url, *expected_status, policy.timeout, cancellation).await,
    }
}

async fn run_exec_check(
    command: &str,
    args: &[String],
    timeout: Duration,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<CheckOutcome, GuardError> {
    let mut spec = ProcessSpec::new("runtime-guard-check", command);
    spec.args = args.to_vec();
    let (mut child, stdout, stderr) =
        spawn_child(&spec, false).map_err(|error| GuardError::Failed(error.to_string()))?;
    let pid = child
        .id()
        .ok_or_else(|| GuardError::Failed("check child has no PID".into()))?;
    let Some((pgid, generation)) = crate::lifecycle::spawned_identity(pid) else {
        let _ = child.start_kill();
        let _ = child.wait().await;
        return Err(GuardError::Failed(
            "check child has no verifiable process-group identity".into(),
        ));
    };
    let handle = ChildHandle {
        process_id: uuid::Uuid::new_v4(),
        pid,
        pgid: Some(pgid),
        generation: Some(generation),
        started_at: chrono::Utc::now(),
    };
    let stdout_pump = tokio::spawn(capture_output(stdout));
    let stderr_pump = tokio::spawn(capture_output(stderr));
    enum Cause {
        Exited(std::process::ExitStatus),
        TimedOut,
        Cancelled,
    }
    let cancellation_requested = async {
        if !*cancellation.borrow() {
            let _ = cancellation.changed().await;
        }
    };
    let cause = tokio::select! {
        status = child.wait() => Cause::Exited(status.map_err(|error| GuardError::Failed(error.to_string()))?),
        _ = cancellation_requested => Cause::Cancelled,
        _ = tokio::time::sleep(timeout) => Cause::TimedOut,
    };

    let mut cleanup_failure = None;
    let status = match cause {
        Cause::Exited(status) => {
            // A shell-less check can still leave descendants holding its pipes.
            // Reap a remaining dedicated group before joining the captures.
            match process_group_exists(pgid) {
                Ok(true) => {
                    if let Err(error) = crate::shutdown::terminate_owned_process_group(
                        &handle,
                        SIGTERM,
                        CHECK_CLEANUP_GRACE,
                        Some(&mut child),
                    )
                    .await
                    {
                        cleanup_failure = Some(error.to_string());
                    }
                }
                Ok(false) => {}
                Err(error) => cleanup_failure = Some(error.to_string()),
            }
            status
        }
        Cause::TimedOut | Cause::Cancelled => match crate::shutdown::terminate_owned_process_group(
            &handle,
            SIGTERM,
            CHECK_CLEANUP_GRACE,
            Some(&mut child),
        )
        .await
        {
            Ok(()) => child
                .wait()
                .await
                .map_err(|error| GuardError::Failed(error.to_string()))?,
            Err(error) => {
                cleanup_failure = Some(error.to_string());
                match tokio::time::timeout(CHECK_DRAIN_TIMEOUT, child.wait()).await {
                    Ok(Ok(status)) => status,
                    Ok(Err(wait_error)) => return Err(GuardError::Failed(wait_error.to_string())),
                    Err(_) => {
                        return Ok(CheckOutcome::CleanupFailed {
                            cause: cleanup_failure
                                .unwrap_or_else(|| "check group cleanup timed out".into()),
                            diagnostics: collect_diagnostics(stdout_pump, stderr_pump).await,
                        })
                    }
                }
            }
        },
    };
    let diagnostics = collect_diagnostics(stdout_pump, stderr_pump).await;
    if let Some(cause) = cleanup_failure {
        return Ok(CheckOutcome::CleanupFailed { cause, diagnostics });
    }
    Ok(match cause {
        Cause::TimedOut => CheckOutcome::TimedOut { diagnostics },
        Cause::Cancelled => CheckOutcome::Cancelled { diagnostics },
        Cause::Exited(_) if status.success() => CheckOutcome::Succeeded {
            status: status.code().map(CheckStatus::ExitCode),
            diagnostics,
        },
        Cause::Exited(_) => CheckOutcome::Failed {
            status: status.code().map(CheckStatus::ExitCode),
            diagnostics,
        },
    })
}

async fn capture_output<R>(reader: Option<R>) -> CapturedOutput
where
    R: AsyncRead + Unpin,
{
    let Some(mut reader) = reader else {
        return CapturedOutput::default();
    };
    let mut bytes = Vec::with_capacity(CHECK_OUTPUT_LIMIT_BYTES.min(4096));
    let mut buffer = [0_u8; 4096];
    let mut truncated = false;
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) => {
                let available = CHECK_OUTPUT_LIMIT_BYTES.saturating_sub(bytes.len());
                let kept = available.min(read);
                bytes.extend_from_slice(&buffer[..kept]);
                truncated |= kept < read;
            }
            Err(error) => {
                return CapturedOutput {
                    value: String::from_utf8_lossy(&bytes).into_owned(),
                    truncated,
                    error: Some(error.to_string()),
                };
            }
        }
    }
    CapturedOutput {
        value: String::from_utf8_lossy(&bytes).into_owned(),
        truncated,
        error: None,
    }
}

#[derive(Default)]
struct CapturedOutput {
    value: String,
    truncated: bool,
    error: Option<String>,
}

async fn collect_diagnostics(
    mut stdout: tokio::task::JoinHandle<CapturedOutput>,
    mut stderr: tokio::task::JoinHandle<CapturedOutput>,
) -> CheckDiagnostics {
    let stdout = match tokio::time::timeout(CHECK_DRAIN_TIMEOUT, &mut stdout).await {
        Ok(Ok(captured)) => captured,
        Ok(Err(error)) => CapturedOutput {
            error: Some(error.to_string()),
            ..Default::default()
        },
        Err(_) => {
            stdout.abort();
            CapturedOutput {
                error: Some("stdout drain timed out".into()),
                ..Default::default()
            }
        }
    };
    let stderr = match tokio::time::timeout(CHECK_DRAIN_TIMEOUT, &mut stderr).await {
        Ok(Ok(captured)) => captured,
        Ok(Err(error)) => CapturedOutput {
            error: Some(error.to_string()),
            ..Default::default()
        },
        Err(_) => {
            stderr.abort();
            CapturedOutput {
                error: Some("stderr drain timed out".into()),
                ..Default::default()
            }
        }
    };
    CheckDiagnostics {
        stdout: append_capture_error(stdout.value, stdout.error),
        stderr: append_capture_error(stderr.value, stderr.error),
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    }
}

fn append_capture_error(mut output: String, error: Option<String>) -> String {
    if let Some(error) = error {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str("[capture error: ");
        output.push_str(&error);
        output.push(']');
    }
    output
}

async fn run_tcp_check(
    host: &str,
    port: u16,
    timeout: Duration,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<CheckOutcome, GuardError> {
    let target = format!("{host}:{port}");
    let cancellation_requested = async {
        if !*cancellation.borrow() {
            let _ = cancellation.changed().await;
        }
    };
    let diagnostics = CheckDiagnostics::default();
    tokio::select! {
        result = tokio::net::TcpStream::connect(&target) => match result {
            Ok(_) => Ok(CheckOutcome::Succeeded { status: None, diagnostics }),
            Err(error) => Ok(CheckOutcome::Failed { status: None, diagnostics: CheckDiagnostics { stderr: error.to_string(), ..diagnostics } }),
        },
        _ = cancellation_requested => Ok(CheckOutcome::Cancelled { diagnostics }),
        _ = tokio::time::sleep(timeout) => Ok(CheckOutcome::TimedOut { diagnostics }),
    }
}

async fn run_http_check(
    url: &str,
    expected_status: u16,
    timeout: Duration,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<CheckOutcome, GuardError> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| GuardError::Failed(error.to_string()))?;
    let request = client.get(url).send();
    let cancellation_requested = async {
        if !*cancellation.borrow() {
            let _ = cancellation.changed().await;
        }
    };
    let diagnostics = CheckDiagnostics::default();
    tokio::select! {
        response = request => match response {
            Ok(response) => {
                let status = response.status().as_u16();
                if status == expected_status {
                    Ok(CheckOutcome::Succeeded { status: Some(CheckStatus::HttpStatus(status)), diagnostics })
                } else {
                    Ok(CheckOutcome::Failed { status: Some(CheckStatus::HttpStatus(status)), diagnostics })
                }
            }
            Err(error) => Ok(CheckOutcome::Failed { status: None, diagnostics: CheckDiagnostics { stderr: error.to_string(), ..diagnostics } }),
        },
        _ = cancellation_requested => Ok(CheckOutcome::Cancelled { diagnostics }),
        _ = tokio::time::sleep(timeout) => Ok(CheckOutcome::TimedOut { diagnostics }),
    }
}

pub(crate) fn register_watch(
    registry: &mut GuardRegistry,
    policy: &WatchPolicy,
    supervisor_log_root: &Path,
) -> Result<WatchRegistrationId, GuardError> {
    let mut normalized = policy.clone();
    normalized
        .exclusions
        .push(supervisor_log_root.to_path_buf());
    let snapshot = snapshot_roots(&normalized)?;
    let (sender, receiver) = mpsc::sync_channel(WATCH_CHANNEL_CAPACITY);
    let channel_overflow = Arc::new(AtomicBool::new(false));
    let overflow = channel_overflow.clone();
    let mut watcher = RecommendedWatcher::new(
        move |event| match sender.try_send(event) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => overflow.store(true, Ordering::SeqCst),
            Err(mpsc::TrySendError::Disconnected(_)) => {}
        },
        Config::default().with_follow_symlinks(false),
    )
    .map_err(|error| GuardError::Failed(error.to_string()))?;
    let mode = if normalized.recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };
    for root in &normalized.roots {
        watcher.watch(root, mode).map_err(|error| {
            GuardError::Failed(format!("cannot watch {}: {error}", root.display()))
        })?;
    }
    let registration = WatchRegistrationId(uuid::Uuid::new_v4());
    registry.watches.insert(
        registration,
        WatchEntry {
            _watcher: watcher,
            receiver,
            channel_overflow,
            policy: normalized,
            pending_paths: BTreeSet::new(),
            pending_since: None,
            snapshot,
        },
    );
    Ok(registration)
}

pub(crate) fn read_watch(
    registry: &mut GuardRegistry,
    registration: WatchRegistrationId,
) -> Result<WatchObservation, GuardError> {
    let entry = registry
        .watches
        .get_mut(&registration)
        .ok_or_else(|| GuardError::Failed("watch registration is not active".into()))?;
    let mut rescan_reason = if entry.channel_overflow.swap(false, Ordering::SeqCst) {
        Some(WatchRescanReason::ChannelOverflow)
    } else {
        None
    };
    while let Ok(event) = entry.receiver.try_recv() {
        match event {
            Ok(event) if matches!(event.kind, EventKind::Access(_)) => {}
            Ok(event) if matches!(event.kind, EventKind::Other) => {
                rescan_reason = Some(WatchRescanReason::BackendOverflow);
            }
            Ok(event) => {
                for path in event
                    .paths
                    .into_iter()
                    .filter(|path| should_include_path(path, &entry.policy))
                {
                    if entry.pending_paths.len() >= WATCH_BATCH_PATH_LIMIT {
                        rescan_reason = Some(WatchRescanReason::ChannelOverflow);
                        break;
                    }
                    entry.pending_paths.insert(path);
                }
                entry.pending_since.get_or_insert_with(Instant::now);
            }
            Err(_) => rescan_reason = Some(WatchRescanReason::BackendRestart),
        }
    }
    if let Some(reason) = rescan_reason {
        let fresh = snapshot_roots(&entry.policy)?;
        let changed_paths = snapshot_diff(&entry.snapshot, &fresh);
        entry.snapshot = fresh;
        entry.pending_paths.clear();
        entry.pending_since = None;
        return Ok(WatchObservation {
            changed_paths,
            rescan_reason: Some(reason),
        });
    }
    if entry
        .pending_since
        .is_some_and(|since| since.elapsed() < entry.policy.debounce)
    {
        return Ok(WatchObservation::default());
    }
    let changed_paths = std::mem::take(&mut entry.pending_paths)
        .into_iter()
        .collect();
    entry.pending_since = None;
    Ok(WatchObservation {
        changed_paths,
        rescan_reason: None,
    })
}

pub(crate) fn stop_watch(
    registry: &mut GuardRegistry,
    registration: WatchRegistrationId,
) -> Result<(), GuardError> {
    registry
        .watches
        .remove(&registration)
        .map(|_| ())
        .ok_or_else(|| GuardError::Failed("watch registration is not active".into()))
}

fn should_include_path(path: &Path, policy: &WatchPolicy) -> bool {
    policy.roots.iter().any(|root| path.starts_with(root))
        && !policy
            .exclusions
            .iter()
            .any(|excluded| path.starts_with(excluded))
        && std::fs::symlink_metadata(path)
            .map(|metadata| !metadata.file_type().is_symlink())
            .unwrap_or(true)
}

fn snapshot_roots(policy: &WatchPolicy) -> Result<Snapshot, GuardError> {
    let mut snapshot = Snapshot::new();
    for root in &policy.roots {
        snapshot_path(root, policy, &mut snapshot)?;
    }
    Ok(snapshot)
}

fn snapshot_path(
    path: &Path,
    policy: &WatchPolicy,
    snapshot: &mut Snapshot,
) -> Result<(), GuardError> {
    if snapshot.len() >= WATCH_RESCAN_ENTRY_LIMIT {
        return Err(GuardError::Failed(format!(
            "watch rescan exceeds {WATCH_RESCAN_ENTRY_LIMIT} entries"
        )));
    }
    if policy
        .exclusions
        .iter()
        .any(|excluded| path.starts_with(excluded))
    {
        return Ok(());
    }
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(GuardError::Failed(format!(
                "cannot inspect {}: {error}",
                path.display()
            )))
        }
    };
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    let is_dir = metadata.is_dir();
    snapshot.insert(
        path.to_path_buf(),
        FileStamp {
            is_dir,
            len: metadata.len(),
            modified: metadata.modified().ok(),
        },
    );
    if is_dir && policy.recursive {
        let entries = std::fs::read_dir(path).map_err(|error| {
            GuardError::Failed(format!("cannot scan {}: {error}", path.display()))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| GuardError::Failed(error.to_string()))?;
            snapshot_path(&entry.path(), policy, snapshot)?;
        }
    }
    Ok(())
}

fn snapshot_diff(previous: &Snapshot, current: &Snapshot) -> Vec<PathBuf> {
    let mut changed = BTreeSet::new();
    for (path, stamp) in current {
        if previous.get(path) != Some(stamp) {
            changed.insert(path.clone());
        }
    }
    for path in previous.keys().filter(|path| !current.contains_key(*path)) {
        changed.insert(path.clone());
    }
    changed.into_iter().take(WATCH_BATCH_PATH_LIMIT).collect()
}
