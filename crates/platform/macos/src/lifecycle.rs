//! `MacLifecycle` — Direct-mode spawn/probe/reap for macOS (Unix spawn + setsid
//! groups; reconciliation compensates for the absence of a subreaper).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::broadcast;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use uuid::Uuid;

use my_supervisor_core::domain::{
    ChildHandle, JobRunId, JobRunState, LogLine, LogStream, ProcessResourceUsage, ProcessSpec,
};
use my_supervisor_core::ports::lifecycle::{
    Aliveness, CheckOutcome, CleanupTicket, GuardError, LifecycleController,
    OwnedGroupResourceUsage, ProbeError, ReapError, SpawnError, TransientCleanupStage,
    TransientCompletion, TransientOutcome, WatchObservation, WatchRegistrationId,
};
use my_supervisor_core::ports::{LogSink, LogTail};

use crate::guards::GuardRegistry;
use crate::signals::{
    matches_handle, process_exists, process_group_exists, process_identity, SIGTERM,
};
use crate::spawn::{
    attach_pumps, spawn_child, spawn_detached_child, DetachedChild, DetachedHelperPaths,
    DetachedTestControls, LogTarget,
};

const FILE_FOLLOW_CAPACITY: usize = 10_256;
const FILE_FOLLOW_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

pub(crate) fn spawned_identity(pid: u32) -> Option<(u32, String)> {
    for _ in 0..20 {
        if let Ok(identity) = process_identity(pid) {
            if identity.pgid == pid && !identity.is_zombie() {
                return Some((identity.pgid, identity.generation));
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    None
}

async fn read_log_tail(
    path: &Path,
    limit: usize,
    stream: LogStream,
) -> Result<Vec<LogLine>, ProbeError> {
    let contents = match tokio::fs::read_to_string(path).await {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(ProbeError::Failed(error.to_string())),
    };
    let lines: Vec<&str> = contents.lines().collect();
    let start = if limit == 0 {
        0
    } else {
        lines.len().saturating_sub(limit)
    };
    Ok(lines[start..]
        .iter()
        // Raw legacy files contain no per-line time or inter-stream order.  A
        // fixed epoch makes the missing timestamp explicit and keeps them out
        // of every normal `since` request instead of fabricating read time.
        .map(|line| LogLine {
            sequence: 0,
            timestamp: DateTime::UNIX_EPOCH,
            stream,
            line: (*line).to_owned(),
        })
        .collect())
}

fn journal_name(process_name: &str) -> String {
    process_name
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn legacy_name(process_name: &str) -> String {
    process_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn parse_journal_line(encoded: &str) -> Option<LogLine> {
    let value = serde_json::from_str::<serde_json::Value>(encoded).ok()?;
    let timestamp = value
        .get("timestamp")?
        .as_str()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())?
        .with_timezone(&Utc);
    Some(LogLine {
        sequence: value.get("sequence")?.as_u64()?,
        timestamp,
        stream: match value.get("stream").and_then(|value| value.as_str()) {
            Some("stderr") => LogStream::Stderr,
            Some("system") => LogStream::System,
            _ => LogStream::Stdout,
        },
        line: value.get("line")?.as_str()?.to_owned(),
    })
}

#[derive(Deserialize)]
struct DetachedSegmentMeta {
    filename: String,
    first_sequence: u64,
    last_sequence: u64,
}

#[derive(Deserialize)]
struct DetachedJournalManifest {
    high_watermark: u64,
    active_start_sequence: u64,
    #[serde(default)]
    sealed_segments: Vec<DetachedSegmentMeta>,
}

fn detached_manifest_path(journal: &Path) -> PathBuf {
    journal.with_extension("manifest.json")
}

async fn read_journal(path: &Path) -> Result<Vec<LogLine>, ProbeError> {
    let contents = match tokio::fs::read_to_string(path).await {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(ProbeError::Failed(format!("{}: {error}", path.display()))),
    };
    Ok(contents.lines().filter_map(parse_journal_line).collect())
}

/// Detached proxy owns the physical journal and writes this manifest.  The
/// lifecycle is a reader only: it selects the cursor-overlapping sealed files
/// plus the active file and never rotates/deletes the proxy's output.
async fn read_detached_journal(
    journal: &Path,
    after_sequence: Option<u64>,
) -> Result<(Vec<LogLine>, u64, Option<u64>), ProbeError> {
    let manifest_path = detached_manifest_path(journal);
    let manifest = tokio::fs::read(&manifest_path)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<DetachedJournalManifest>(&bytes).ok());
    let Some(manifest) = manifest else {
        let lines = read_journal(journal).await?;
        let high = lines.iter().map(|line| line.sequence).max().unwrap_or(0);
        let earliest = lines.first().map(|line| line.sequence);
        return Ok((lines, high, earliest));
    };

    // The proxy commits a row to the active JSONL file before atomically
    // replacing its manifest.  A reader can therefore observe a newer file
    // with an older manifest (or the inverse during rollover).  Never expose
    // the manifest's cursor beyond the rows observed in that same snapshot:
    // doing so could make a reconnect skip an existing sequence.  A short
    // cooperative retry handles an in-flight rename without changing caller
    // timing or weakening the cursor contract.
    let mut manifest = manifest;
    for attempt in 0..8 {
        let mut by_sequence = BTreeMap::new();
        for segment in &manifest.sealed_segments {
            if after_sequence.is_some_and(|cursor| segment.last_sequence <= cursor) {
                continue;
            }
            for line in read_journal(&journal.with_file_name(&segment.filename)).await? {
                by_sequence.entry(line.sequence).or_insert(line);
            }
        }
        for line in read_journal(journal).await? {
            by_sequence.entry(line.sequence).or_insert(line);
        }
        let lines = by_sequence.into_values().collect::<Vec<_>>();
        let observed_high_watermark = lines
            .last()
            .map(|line| line.sequence)
            .unwrap_or(0)
            .max(manifest.high_watermark.min(after_sequence.unwrap_or(0)));
        let earliest_retained_sequence = manifest
            .sealed_segments
            .first()
            .map(|segment| segment.first_sequence)
            .or_else(|| {
                (manifest.high_watermark >= manifest.active_start_sequence)
                    .then_some(manifest.active_start_sequence)
            });
        if observed_high_watermark >= manifest.high_watermark || attempt == 7 {
            return Ok((lines, observed_high_watermark, earliest_retained_sequence));
        }
        tokio::task::yield_now().await;
        if let Ok(bytes) = tokio::fs::read(&manifest_path).await {
            if let Ok(updated) = serde_json::from_slice::<DetachedJournalManifest>(&bytes) {
                manifest = updated;
            }
        }
    }
    unreachable!("the bounded detached-journal snapshot loop always returns");
}

fn journal_tail(
    mut lines: Vec<LogLine>,
    high_watermark: u64,
    earliest_retained_sequence: Option<u64>,
    limit: usize,
    since: Option<DateTime<Utc>>,
    after_sequence: Option<u64>,
) -> LogTail {
    let cursor_expired = after_sequence.is_some_and(|sequence| {
        earliest_retained_sequence.is_some_and(|earliest| sequence.saturating_add(1) < earliest)
    });
    lines.retain(|line| since.is_none_or(|value| line.timestamp >= value));
    lines.retain(|line| after_sequence.is_none_or(|value| line.sequence > value));
    let truncated = limit > 0 && lines.len() > limit;
    if truncated {
        lines = lines.split_off(lines.len() - limit);
    }
    LogTail {
        lines,
        truncated,
        high_watermark,
        next_sequence: high_watermark.saturating_add(1),
        earliest_retained_sequence,
        cursor_expired,
    }
}

#[cfg(test)]
fn follow_log_file(path: PathBuf, stream: LogStream, tx: broadcast::Sender<LogLine>) {
    tokio::spawn(async move {
        let mut offset = tokio::fs::metadata(&path)
            .await
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let mut partial_line = String::new();
        let mut interval = tokio::time::interval(FILE_FOLLOW_INTERVAL);
        loop {
            interval.tick().await;
            if tx.receiver_count() == 0 {
                break;
            }
            let Ok(metadata) = tokio::fs::metadata(&path).await else {
                continue;
            };
            if metadata.len() < offset {
                offset = 0;
                partial_line.clear();
            }
            if metadata.len() == offset {
                continue;
            }
            let Ok(mut file) = tokio::fs::File::open(&path).await else {
                continue;
            };
            if file.seek(std::io::SeekFrom::Start(offset)).await.is_err() {
                continue;
            }
            let mut appended = Vec::new();
            if file.read_to_end(&mut appended).await.is_err() {
                continue;
            }
            offset = offset.saturating_add(appended.len() as u64);
            partial_line.push_str(&String::from_utf8_lossy(&appended));
            while let Some(newline_position) = partial_line.find('\n') {
                let mut line = partial_line.drain(..=newline_position).collect::<String>();
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
                let _ = tx.send(LogLine::now(stream, line));
            }
        }
    });
}

fn follow_journal_file(path: PathBuf, tx: broadcast::Sender<LogLine>, initial_offset: u64) {
    tokio::spawn(async move {
        let mut offset = initial_offset;
        let mut partial_line = String::new();
        let mut interval = tokio::time::interval(FILE_FOLLOW_INTERVAL);
        loop {
            interval.tick().await;
            if tx.receiver_count() == 0 {
                break;
            }
            let Ok(metadata) = tokio::fs::metadata(&path).await else {
                continue;
            };
            if metadata.len() < offset {
                offset = 0;
                partial_line.clear();
            }
            if metadata.len() == offset {
                continue;
            }
            let Ok(mut file) = tokio::fs::File::open(&path).await else {
                continue;
            };
            if file.seek(std::io::SeekFrom::Start(offset)).await.is_err() {
                continue;
            }
            let mut appended = Vec::new();
            if file.read_to_end(&mut appended).await.is_err() {
                continue;
            }
            offset = offset.saturating_add(appended.len() as u64);
            partial_line.push_str(&String::from_utf8_lossy(&appended));
            while let Some(newline_position) = partial_line.find('\n') {
                let encoded = partial_line.drain(..=newline_position).collect::<String>();
                if let Some(line) = parse_journal_line(encoded.trim_end_matches(['\r', '\n'])) {
                    let _ = tx.send(line);
                }
            }
        }
    });
}

async fn sample_resource_usage(handle: &ChildHandle) -> Result<ProcessResourceUsage, ProbeError> {
    if !matches_handle(handle) {
        return Err(ProbeError::Failed(
            "process identity no longer matches the recorded handle".into(),
        ));
    }
    let pid = handle.pid;
    let output = tokio::process::Command::new("ps")
        .env("LC_ALL", "C")
        .args(["-o", "%cpu=,rss=", "-p", &pid.to_string()])
        .output()
        .await
        .map_err(|error| ProbeError::Failed(error.to_string()))?;
    if !output.status.success() {
        return Err(ProbeError::Failed(format!("process {pid} not found")));
    }
    let output = String::from_utf8_lossy(&output.stdout);
    let mut values = output.split_whitespace();
    let cpu_percent = values
        .next()
        .ok_or_else(|| ProbeError::Failed(format!("missing CPU usage for {pid}")))?
        .parse::<f32>()
        .map_err(|error| ProbeError::Failed(error.to_string()))?;
    let memory_kilobytes = values
        .next()
        .ok_or_else(|| ProbeError::Failed(format!("missing memory usage for {pid}")))?
        .parse::<u64>()
        .map_err(|error| ProbeError::Failed(error.to_string()))?;
    // The `ps` sample is not an identity proof.  Reject it if the process
    // exited or its PID was recycled while the command was running.
    if !matches_handle(handle) {
        return Err(ProbeError::Failed(
            "process identity changed while collecting resource usage".into(),
        ));
    }
    Ok(ProcessResourceUsage {
        cpu_percent,
        memory_bytes: memory_kilobytes.saturating_mul(1024),
    })
}

pub struct MacLifecycle {
    log_sink: Arc<dyn LogSink>,
    log_dir: PathBuf,
    detached_helpers: Option<DetachedHelperPaths>,
    detached_test_controls: DetachedTestControls,
    children: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    transient_children: Arc<Mutex<HashMap<Uuid, TransientChild>>>,
    guards: Arc<Mutex<GuardRegistry>>,
}

struct TransientChild {
    child: tokio::process::Child,
    pumps: Vec<JoinHandle<Result<(), my_supervisor_core::ports::LogError>>>,
    started_at: chrono::DateTime<Utc>,
}

impl MacLifecycle {
    pub fn new(log_sink: Arc<dyn LogSink>, log_dir: PathBuf) -> Self {
        MacLifecycle {
            log_sink,
            log_dir,
            detached_helpers: None,
            detached_test_controls: DetachedTestControls::default(),
            children: Arc::new(Mutex::new(HashMap::new())),
            transient_children: Arc::new(Mutex::new(HashMap::new())),
            guards: Arc::new(Mutex::new(GuardRegistry::default())),
        }
    }

    /// Construct a lifecycle whose detached helpers were selected and
    /// validated by the production host at composition time.
    pub fn with_detached_helpers(
        log_sink: Arc<dyn LogSink>,
        log_dir: PathBuf,
        detached_helpers: DetachedHelperPaths,
    ) -> Self {
        MacLifecycle {
            log_sink,
            log_dir,
            detached_helpers: Some(detached_helpers),
            detached_test_controls: DetachedTestControls::default(),
            children: Arc::new(Mutex::new(HashMap::new())),
            transient_children: Arc::new(Mutex::new(HashMap::new())),
            guards: Arc::new(Mutex::new(GuardRegistry::default())),
        }
    }

    /// Construct a detached-test lifecycle with fault controls supplied by
    /// the fixture rather than by target process input.
    pub fn with_detached_helpers_and_test_controls(
        log_sink: Arc<dyn LogSink>,
        log_dir: PathBuf,
        detached_helpers: DetachedHelperPaths,
        detached_test_controls: DetachedTestControls,
    ) -> Self {
        MacLifecycle {
            log_sink,
            log_dir,
            detached_helpers: Some(detached_helpers),
            detached_test_controls,
            children: Arc::new(Mutex::new(HashMap::new())),
            transient_children: Arc::new(Mutex::new(HashMap::new())),
            guards: Arc::new(Mutex::new(GuardRegistry::default())),
        }
    }

    fn detached_journal_path(&self, process_name: &str) -> PathBuf {
        self.log_dir
            .join(format!("direct-{}.jsonl", journal_name(process_name)))
    }

    fn legacy_detached_log_paths(&self, process_name: &str) -> (PathBuf, PathBuf) {
        let name = legacy_name(process_name);
        (
            self.log_dir.join(format!("direct-{name}.stdout.log")),
            self.log_dir.join(format!("direct-{name}.stderr.log")),
        )
    }

    fn spawn_common(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> {
        let (child, stdout, stderr) = spawn_child(spec, false)?;
        attach_pumps(
            stdout,
            stderr,
            &self.log_sink,
            LogTarget::Process(spec.name.clone()),
        );
        self.track_child(spec, child)
    }

    fn spawn_detached_common(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> {
        let helpers = self
            .detached_helpers
            .as_ref()
            .ok_or_else(|| SpawnError::Io {
                name: spec.name.clone(),
                message: "detached helpers were not configured by the runtime host".into(),
            })?;
        let child = spawn_detached_child(
            spec,
            &self.detached_journal_path(&spec.name),
            helpers,
            &self.detached_test_controls,
        )?;
        self.track_detached_child(spec, child)
    }

    fn track_child(
        &self,
        spec: &ProcessSpec,
        mut child: tokio::process::Child,
    ) -> Result<ChildHandle, SpawnError> {
        let process_id = Uuid::new_v4();
        let pid = child.id().unwrap_or(0);
        let started_at = Utc::now();
        // `setsid` must have completed before we publish a supervisor handle.
        // A missing identity is unsafe because a later PID reuse could target an
        // unrelated group, so the newly spawned child is discarded instead.
        let Some((pgid, generation)) = spawned_identity(pid) else {
            if pid != 0 {
                // SAFETY: this child was just spawned with setsid; targeting its
                // negative pid reclaims its whole still-unpublished group.
                unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
            }
            let _ = child.start_kill();
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
            return Err(SpawnError::Io {
                name: spec.name.clone(),
                message: "spawned child did not expose a verifiable process-group identity".into(),
            });
        };

        let alive = Arc::new(AtomicBool::new(true));
        self.children
            .lock()
            .unwrap()
            .insert(process_id, alive.clone());

        let children = self.children.clone();
        let sink = self.log_sink.clone();
        let name = spec.name.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            alive.store(false, Ordering::SeqCst);
            children.lock().unwrap().remove(&process_id);
            let note = match status {
                Ok(s) => format!("process exited: {s}"),
                Err(e) => format!("process wait failed: {e}"),
            };
            let _ = sink
                .append(&name, LogLine::now(LogStream::System, note))
                .await;
        });

        Ok(ChildHandle {
            process_id,
            pid,
            pgid: Some(pgid),
            generation: Some(generation),
            started_at,
        })
    }

    fn track_detached_child(
        &self,
        spec: &ProcessSpec,
        mut detached: DetachedChild,
    ) -> Result<ChildHandle, SpawnError> {
        let process_id = Uuid::new_v4();
        let pid = detached.proxy.id().unwrap_or(0);
        let started_at = Utc::now();
        let Some((pgid, generation)) = spawned_identity(pid) else {
            if pid != 0 {
                // SAFETY: this proxy was just spawned with setsid; its group
                // has not yet been published to any caller.
                unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
            }
            let _ = detached.proxy.start_kill();
            let mut reapers = detached.reapers;
            tokio::spawn(async move {
                let _ = detached.proxy.wait().await;
                let _ = tokio::task::spawn_blocking(move || {
                    for reaper in &mut reapers {
                        let _ = reaper.wait();
                    }
                })
                .await;
            });
            return Err(SpawnError::Io {
                name: spec.name.clone(),
                message: "detached proxy did not expose a verifiable process-group identity".into(),
            });
        };

        let alive = Arc::new(AtomicBool::new(true));
        self.children
            .lock()
            .unwrap()
            .insert(process_id, alive.clone());

        let children = self.children.clone();
        let sink = self.log_sink.clone();
        let name = spec.name.clone();
        tokio::spawn(async move {
            let proxy_status = detached.proxy.wait().await;
            // The proxy's `Child` handle alone is insufficient: each reaper is
            // also a direct child of this runtime and must be waited after
            // normal completion, takeover, or a proxy crash to prevent zombies.
            let reaper_status = tokio::task::spawn_blocking(move || {
                let mut failures = Vec::new();
                for reaper in &mut detached.reapers {
                    if let Err(error) = reaper.wait() {
                        failures.push(error.to_string());
                    }
                }
                failures
            })
            .await;
            alive.store(false, Ordering::SeqCst);
            children.lock().unwrap().remove(&process_id);
            let note = match (proxy_status, reaper_status) {
                (Ok(status), Ok(failures)) if failures.is_empty() => {
                    format!("detached proxy and cleanup owners exited: {status}")
                }
                (Ok(status), Ok(failures)) => {
                    format!(
                        "detached proxy exited: {status}; cleanup owner waits failed: {}",
                        failures.join(", ")
                    )
                }
                (Err(error), _) => format!("detached proxy wait failed: {error}"),
                (_, Err(error)) => format!("detached cleanup owner waiter failed: {error}"),
            };
            let _ = sink
                .append(&name, LogLine::now(LogStream::System, note))
                .await;
        });

        Ok(ChildHandle {
            process_id,
            pid,
            pgid: Some(pgid),
            generation: Some(generation),
            started_at,
        })
    }
}

#[async_trait]
impl LifecycleController for MacLifecycle {
    async fn spawn_tied(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> {
        self.spawn_common(spec)
    }

    async fn spawn_detached(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> {
        self.spawn_detached_common(spec)
    }

    async fn probe_alive(&self, handle: &ChildHandle) -> Result<Aliveness, ProbeError> {
        if !process_exists(handle.pid).map_err(|error| ProbeError::Failed(error.to_string()))? {
            Ok(Aliveness::Dead)
        } else if matches_handle(handle) {
            Ok(Aliveness::Alive)
        } else {
            Err(ProbeError::Failed(
                "live process identity could not be verified".into(),
            ))
        }
    }

    async fn tail_detached_logs(
        &self,
        spec: &ProcessSpec,
        lines: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
        known_process_names: &[String],
    ) -> Result<LogTail, ProbeError> {
        let journal = self.detached_journal_path(&spec.name);
        let (journal_lines, high_watermark, earliest_retained_sequence) =
            read_detached_journal(&journal, after_sequence).await?;
        if !journal_lines.is_empty() || tokio::fs::try_exists(&journal).await.unwrap_or(false) {
            return Ok(journal_tail(
                journal_lines,
                high_watermark,
                earliest_retained_sequence,
                lines,
                since,
                after_sequence,
            ));
        }
        // A legacy sanitized basename is readable only when the repository's
        // complete process set proves it unique.  It has no recoverable cursor
        // or timestamp, so it is deliberately excluded from `since`/cursor.
        if since.is_some()
            || after_sequence.is_some()
            || known_process_names
                .iter()
                .filter(|name| legacy_name(name) == legacy_name(&spec.name))
                .count()
                != 1
        {
            return Ok(LogTail::default());
        }
        let (stdout, stderr) = self.legacy_detached_log_paths(&spec.name);
        let mut legacy = read_log_tail(&stdout, 0, LogStream::Stdout).await?;
        legacy.extend(read_log_tail(&stderr, 0, LogStream::Stderr).await?);
        let truncated = lines > 0 && legacy.len() > lines;
        if truncated {
            legacy = legacy.split_off(legacy.len() - lines);
        }
        Ok(LogTail {
            lines: legacy,
            truncated,
            high_watermark: 0,
            next_sequence: 1,
            earliest_retained_sequence: None,
            cursor_expired: false,
        })
    }

    async fn subscribe_detached_logs(
        &self,
        spec: &ProcessSpec,
    ) -> Result<broadcast::Receiver<LogLine>, ProbeError> {
        let journal = self.detached_journal_path(&spec.name);
        let (tx, receiver) = broadcast::channel(FILE_FOLLOW_CAPACITY);
        let initial_offset = tokio::fs::metadata(&journal)
            .await
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        follow_journal_file(journal, tx, initial_offset);
        Ok(receiver)
    }

    async fn resource_usage(
        &self,
        handle: &ChildHandle,
    ) -> Result<ProcessResourceUsage, ProbeError> {
        sample_resource_usage(handle).await
    }

    async fn owned_group_resource_usage(
        &self,
        handle: &ChildHandle,
    ) -> Result<OwnedGroupResourceUsage, GuardError> {
        crate::guards::sample_owned_group_resource_usage(handle).await
    }

    async fn run_check(
        &self,
        policy: &my_supervisor_core::domain::CheckPolicy,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<CheckOutcome, GuardError> {
        crate::guards::run_check(policy, cancellation).await
    }

    async fn register_watch(
        &self,
        policy: &my_supervisor_core::domain::WatchPolicy,
    ) -> Result<WatchRegistrationId, GuardError> {
        crate::guards::register_watch(&mut self.guards.lock().unwrap(), policy, &self.log_dir)
    }

    async fn read_watch(
        &self,
        registration: WatchRegistrationId,
    ) -> Result<WatchObservation, GuardError> {
        crate::guards::read_watch(&mut self.guards.lock().unwrap(), registration)
    }

    async fn stop_watch(&self, registration: WatchRegistrationId) -> Result<(), GuardError> {
        crate::guards::stop_watch(&mut self.guards.lock().unwrap(), registration)
    }

    async fn reap_on_shutdown(&self, handles: &[ChildHandle]) -> Result<(), ReapError> {
        for handle in handles {
            crate::shutdown::terminate_owned_process_group(
                handle,
                SIGTERM,
                std::time::Duration::from_millis(500),
                None,
            )
            .await
            .map_err(|error| ReapError::Failed(error.to_string()))?;
        }
        Ok(())
    }

    async fn start_transient(
        &self,
        spec: &ProcessSpec,
        run_id: JobRunId,
    ) -> Result<ChildHandle, SpawnError> {
        let started_at = Utc::now();
        let (mut child, stdout, stderr) = spawn_child(spec, false)?;
        let pid = child.id().ok_or_else(|| SpawnError::Io {
            name: spec.name.clone(),
            message: "spawned transient child has no PID".into(),
        })?;
        let Some((pgid, generation)) = spawned_identity(pid) else {
            // The child is still exclusively owned here. Reap synchronously so
            // PID reuse cannot escape through a detached waiter task.
            unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(SpawnError::Io {
                name: spec.name.clone(),
                message:
                    "spawned transient child did not expose a verifiable process-group identity"
                        .into(),
            });
        };
        let handle = ChildHandle {
            process_id: Uuid::new_v4(),
            pid,
            pgid: Some(pgid),
            generation: Some(generation),
            started_at,
        };
        let pumps = attach_pumps(stdout, stderr, &self.log_sink, LogTarget::Run(run_id));
        self.transient_children.lock().unwrap().insert(
            handle.process_id,
            TransientChild {
                child,
                pumps,
                started_at,
            },
        );
        Ok(handle)
    }

    async fn complete_transient(
        &self,
        handle: &ChildHandle,
        timeout: Option<std::time::Duration>,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<TransientCompletion, SpawnError> {
        let Some(mut transient) = self
            .transient_children
            .lock()
            .unwrap()
            .remove(&handle.process_id)
        else {
            return Ok(TransientCompletion::CleanupPending {
                cause: "transient child ownership is unavailable".into(),
                stage: TransientCleanupStage::TerminateGroup,
                intended_terminal_state: JobRunState::Cancelled,
                outcome: TransientOutcome {
                    started_at: handle.started_at,
                    ended_at: Utc::now(),
                    exit_code: None,
                },
            });
        };

        enum CompletionCause {
            Exited(std::process::ExitStatus),
            TimedOut,
            Cancelled,
        }
        let cancellation_requested = async {
            if !*cancellation.borrow() {
                let _ = cancellation.changed().await;
            }
        };
        let completion_cause = tokio::select! {
            status = transient.child.wait() => CompletionCause::Exited(status.map_err(|error| SpawnError::Io {
                name: handle.pid.to_string(), message: error.to_string()
            })?),
            _ = cancellation_requested => CompletionCause::Cancelled,
            _ = tokio::time::sleep(timeout.unwrap_or_default()), if timeout.is_some() => CompletionCause::TimedOut,
        };

        let (status, terminal_kind) = match completion_cause {
            CompletionCause::Exited(status) => (status, None),
            CompletionCause::TimedOut => {
                match terminate_transient(&mut transient.child, handle).await {
                    Ok(status) => (status, Some(true)),
                    Err(cause) => {
                        let started_at = transient.started_at;
                        self.transient_children
                            .lock()
                            .unwrap()
                            .insert(handle.process_id, transient);
                        return Ok(TransientCompletion::CleanupPending {
                            cause,
                            stage: TransientCleanupStage::TerminateGroup,
                            intended_terminal_state: JobRunState::TimedOut,
                            outcome: TransientOutcome {
                                started_at,
                                ended_at: Utc::now(),
                                exit_code: None,
                            },
                        });
                    }
                }
            }
            CompletionCause::Cancelled => {
                match terminate_transient(&mut transient.child, handle).await {
                    Ok(status) => (status, Some(false)),
                    Err(cause) => {
                        let started_at = transient.started_at;
                        self.transient_children
                            .lock()
                            .unwrap()
                            .insert(handle.process_id, transient);
                        return Ok(TransientCompletion::CleanupPending {
                            cause,
                            stage: TransientCleanupStage::TerminateGroup,
                            intended_terminal_state: JobRunState::Cancelled,
                            outcome: TransientOutcome {
                                started_at,
                                ended_at: Utc::now(),
                                exit_code: None,
                            },
                        });
                    }
                }
            }
        };
        let outcome = TransientOutcome {
            started_at: transient.started_at,
            ended_at: Utc::now(),
            exit_code: status.code(),
        };
        for pump in transient.pumps {
            match pump.await {
                Err(error) => {
                    return Ok(TransientCompletion::CleanupPending {
                        cause: format!("transient output pump did not finish: {error}"),
                        stage: TransientCleanupStage::SealLog,
                        intended_terminal_state: terminal_state(terminal_kind, status.code()),
                        outcome,
                    })
                }
                Ok(Err(error)) => {
                    return Ok(TransientCompletion::CleanupPending {
                        cause: error.to_string(),
                        stage: TransientCleanupStage::SealLog,
                        intended_terminal_state: terminal_state(terminal_kind, status.code()),
                        outcome,
                    })
                }
                Ok(Ok(())) => {}
            }
        }
        Ok(match terminal_kind {
            Some(true) => TransientCompletion::TimedOut(outcome),
            Some(false) => TransientCompletion::Cancelled(outcome),
            None => TransientCompletion::Exited(outcome),
        })
    }

    async fn resume_transient_cleanup(
        &self,
        ticket: &CleanupTicket,
    ) -> Result<TransientCompletion, SpawnError> {
        if self
            .transient_children
            .lock()
            .unwrap()
            .contains_key(&ticket.child.process_id)
        {
            let (sender, mut cancellation) = watch::channel(true);
            drop(sender);
            return self
                .complete_transient(&ticket.child, None, &mut cancellation)
                .await;
        }

        let group_is_gone = ticket
            .child
            .pgid
            .map(|pgid| process_group_exists(pgid).map(|exists| !exists))
            .transpose()
            .map_err(|error| SpawnError::Io {
                name: ticket.child.pid.to_string(),
                message: error.to_string(),
            })?
            .unwrap_or(false);
        if group_is_gone {
            return Ok(terminal_completion(
                ticket.intended_terminal_state,
                ticket.outcome,
            ));
        }
        // A restarted daemon has no `Child` handle, but the ticket retains a
        // verified generation and dedicated PGID.  Reuse the same whole-group
        // primitive as Direct stop instead of retrying forever without an
        // owner.
        match crate::shutdown::terminate_owned_process_group(
            &ticket.child,
            crate::signals::SIGTERM,
            std::time::Duration::from_secs(2),
            None,
        )
        .await
        {
            Ok(()) => {
                return Ok(terminal_completion(
                    ticket.intended_terminal_state,
                    ticket.outcome,
                ))
            }
            Err(error) => {
                return Ok(TransientCompletion::CleanupPending {
                    cause: error.to_string(),
                    stage: TransientCleanupStage::TerminateGroup,
                    intended_terminal_state: ticket.intended_terminal_state,
                    outcome: ticket.outcome,
                })
            }
        }
    }
}

fn terminal_completion(state: JobRunState, outcome: TransientOutcome) -> TransientCompletion {
    match state {
        JobRunState::TimedOut => TransientCompletion::TimedOut(outcome),
        JobRunState::Cancelled => TransientCompletion::Cancelled(outcome),
        _ => TransientCompletion::Exited(outcome),
    }
}

fn terminal_state(terminal_kind: Option<bool>, exit_code: Option<i32>) -> JobRunState {
    match terminal_kind {
        Some(true) => JobRunState::TimedOut,
        Some(false) => JobRunState::Cancelled,
        None if exit_code == Some(0) => JobRunState::Succeeded,
        None => JobRunState::Failed,
    }
}

async fn terminate_transient(
    child: &mut tokio::process::Child,
    handle: &ChildHandle,
) -> Result<std::process::ExitStatus, String> {
    crate::shutdown::terminate_owned_process_group(
        handle,
        SIGTERM,
        std::time::Duration::from_secs(2),
        Some(child),
    )
    .await
    .map_err(|error| error.to_string())?;
    child.wait().await.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{follow_log_file, sample_resource_usage, MacLifecycle, FILE_FOLLOW_CAPACITY};
    use crate::shutdown::UnixShutdown;
    use my_supervisor_core::domain::{ChildHandle, LogStream, ProcessSpec, ShutdownPolicy};
    use my_supervisor_core::ports::{
        LifecycleController, LogSink, ShutdownSignaler, TransientCompletion,
    };
    use my_supervisor_infra_logging::InMemoryLogSink;
    use std::io::Write;
    use std::sync::Arc;
    use tokio::sync::{broadcast, watch};

    fn assert_process_group_reaped(handle: &ChildHandle) {
        let pgid = handle
            .pgid
            .expect("transient child must own a process group");
        // SAFETY: signal 0 only queries whether this dedicated group remains.
        assert_ne!(unsafe { libc::kill(-(pgid as i32), 0) }, 0);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[tokio::test]
    async fn file_follower_emits_only_new_complete_lines() {
        let path = std::env::temp_dir().join(format!(
            "my-supervisor-follow-test-{}-{}.log",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::write(&path, "old line\n").unwrap();
        let (sender, mut receiver) = broadcast::channel(FILE_FOLLOW_CAPACITY);
        follow_log_file(path.clone(), LogStream::Stdout, sender);
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"new line\n").unwrap();
        file.flush().unwrap();
        let line = tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(line.line, "new line");
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn rejects_resource_sampling_without_a_dedicated_matching_group() {
        let pid = std::process::id();
        let usage = sample_resource_usage(&ChildHandle {
            process_id: uuid::Uuid::new_v4(),
            pid,
            pgid: Some(pid),
            generation: super::process_identity(pid)
                .ok()
                .map(|identity| identity.generation),
            started_at: chrono::Utc::now(),
        })
        .await;
        assert!(usage.is_err());
    }

    #[tokio::test]
    async fn verified_group_shutdown_reaps_descendants_and_rejects_stale_handles() {
        let lifecycle = MacLifecycle::new(
            Arc::new(InMemoryLogSink::new()),
            std::env::temp_dir().join(format!("my-supervisor-group-test-{}", uuid::Uuid::new_v4())),
        );
        let mut spec = ProcessSpec::new("group-test", "/bin/sh");
        spec.args = vec!["-c".into(), "sleep 30 & wait".into()];
        let handle = lifecycle.spawn_tied(&spec).await.unwrap();
        assert!(matches!(
            lifecycle.probe_alive(&handle).await.unwrap(),
            my_supervisor_core::ports::Aliveness::Alive
        ));

        let mut stale = handle.clone();
        stale.generation = Some("wrong-generation".into());
        let error = UnixShutdown::new().force_kill(&stale).await.unwrap_err();
        assert!(matches!(
            error,
            my_supervisor_core::ports::SignalError::IdentityMismatch
        ));
        assert!(matches!(
            lifecycle.probe_alive(&handle).await.unwrap(),
            my_supervisor_core::ports::Aliveness::Alive
        ));

        UnixShutdown::new()
            .request_graceful(
                &handle,
                &ShutdownPolicy {
                    signal: my_supervisor_core::domain::ShutdownSignal::Term,
                    grace_period: std::time::Duration::from_millis(100),
                },
            )
            .await
            .unwrap();
        for _ in 0..20 {
            if !super::process_exists(handle.pid).unwrap() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(!super::process_exists(handle.pid).unwrap());
    }

    #[tokio::test]
    async fn transient_cancellation_reaps_the_child_and_joins_output_pumps() {
        let sink = Arc::new(InMemoryLogSink::new());
        let lifecycle = MacLifecycle::new(
            sink.clone(),
            std::env::temp_dir().join(format!(
                "my-supervisor-transient-test-{}",
                uuid::Uuid::new_v4()
            )),
        );
        let run_id = my_supervisor_core::domain::JobRunId::new();
        let mut spec = ProcessSpec::new("transient-cancel", "/bin/sh");
        spec.args = vec![
            "-c".into(),
            "printf 'transient started\\n'; trap 'exit 0' TERM; while :; do sleep 1; done".into(),
        ];
        let handle = lifecycle.start_transient(&spec, run_id).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let (sender, mut cancellation) = watch::channel(false);
        sender.send_replace(true);
        let completion = lifecycle
            .complete_transient(&handle, None, &mut cancellation)
            .await
            .unwrap();

        assert!(matches!(completion, TransientCompletion::Cancelled(_)));
        assert!(!super::process_exists(handle.pid).unwrap());
        assert_process_group_reaped(&handle);
        assert!(!lifecycle
            .transient_children
            .lock()
            .unwrap()
            .contains_key(&handle.process_id));
        assert!(sink
            .tail_run(run_id, 10, None, None)
            .await
            .lines
            .iter()
            .any(|line| line.line == "transient started"));
    }

    #[tokio::test]
    async fn transient_timeout_reaps_the_child_and_joins_output_pumps() {
        let sink = Arc::new(InMemoryLogSink::new());
        let lifecycle = MacLifecycle::new(
            sink.clone(),
            std::env::temp_dir().join(format!(
                "my-supervisor-transient-timeout-{}",
                uuid::Uuid::new_v4()
            )),
        );
        let run_id = my_supervisor_core::domain::JobRunId::new();
        let mut spec = ProcessSpec::new("transient-timeout", "/bin/sh");
        spec.args = vec![
            "-c".into(),
            "printf 'transient started\\n'; trap 'exit 0' TERM; while :; do sleep 1; done".into(),
        ];
        let handle = lifecycle.start_transient(&spec, run_id).await.unwrap();
        let (_sender, mut cancellation) = watch::channel(false);
        let completion = lifecycle
            .complete_transient(
                &handle,
                Some(std::time::Duration::from_millis(50)),
                &mut cancellation,
            )
            .await
            .unwrap();

        assert!(matches!(completion, TransientCompletion::TimedOut(_)));
        assert!(!super::process_exists(handle.pid).unwrap());
        assert_process_group_reaped(&handle);
        assert!(!lifecycle
            .transient_children
            .lock()
            .unwrap()
            .contains_key(&handle.process_id));
        assert!(sink
            .tail_run(run_id, 10, None, None)
            .await
            .lines
            .iter()
            .any(|line| line.line == "transient started"));
    }
}
