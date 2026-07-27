//! Durable, source-local segmented JSONL journals for process and job-run
//! output.  Numeric sequence allocation is independent of physical segment
//! names; the manifest is the small, atomic index used for recovery and tail.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, Mutex as AsyncMutex};

use my_supervisor_core::domain::{JobRunId, LogLine, LogStream};
use my_supervisor_core::ports::{LogError, LogSink, LogTail};

const RING_CAPACITY: usize = 10_000;
const BROADCAST_CAPACITY: usize = 10_256;
const MANIFEST_VERSION: u32 = 1;
const FSYNC_EVERY_LINES: u64 = 64;

/// Internal compatibility policy used by every production durable journal.
/// Callers can inject a smaller value only for bounded evidence/diagnostics;
/// this is intentionally not a public config surface.
#[derive(Debug, Clone)]
pub struct JournalPolicy {
    pub max_segment_bytes: u64,
    pub max_segment_age: Duration,
    pub max_sealed_segments: usize,
    pub max_total_bytes: u64,
}

impl Default for JournalPolicy {
    fn default() -> Self {
        Self {
            max_segment_bytes: 1_048_576,
            max_segment_age: Duration::hours(24),
            max_sealed_segments: 32,
            max_total_bytes: 32 * 1_048_576,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SegmentMeta {
    filename: String,
    first_sequence: u64,
    last_sequence: u64,
    created_at: DateTime<Utc>,
    sealed_at: DateTime<Utc>,
    byte_len: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalManifest {
    version: u32,
    next_sequence: u64,
    high_watermark: u64,
    active_start_sequence: u64,
    active_created_at: DateTime<Utc>,
    active_byte_len: u64,
    #[serde(default)]
    sealed: bool,
    #[serde(default)]
    sealed_segments: Vec<SegmentMeta>,
    /// Paths removed from the logical retained range before physical delete.
    /// Keeping this compact tombstone list makes interrupted cleanup retryable
    /// without resurrecting an expired cursor range during recovery.
    #[serde(default)]
    deleted_segments: Vec<String>,
}

impl JournalManifest {
    fn empty(now: DateTime<Utc>) -> Self {
        Self {
            version: MANIFEST_VERSION,
            next_sequence: 1,
            high_watermark: 0,
            active_start_sequence: 1,
            active_created_at: now,
            active_byte_len: 0,
            sealed: false,
            sealed_segments: Vec::new(),
            deleted_segments: Vec::new(),
        }
    }

    fn earliest_retained_sequence(&self) -> Option<u64> {
        self.sealed_segments
            .first()
            .map(|segment| segment.first_sequence)
            .or_else(|| {
                (self.high_watermark >= self.active_start_sequence)
                    .then_some(self.active_start_sequence)
            })
    }
}

#[derive(Clone)]
struct JournalPaths {
    active: PathBuf,
    manifest: PathBuf,
    segment_prefix: String,
    legacy: Option<PathBuf>,
    is_run: bool,
}

impl JournalPaths {
    fn segment(&self, start_sequence: u64) -> PathBuf {
        self.active.with_file_name(format!(
            "{}segment-{start_sequence:020}.jsonl",
            self.segment_prefix
        ))
    }
}

struct JournalState {
    buffer: VecDeque<LogLine>,
    initialized: bool,
    manifest: Option<JournalManifest>,
    writes_since_sync: u64,
}

struct Journal {
    state: AsyncMutex<JournalState>,
    tx: broadcast::Sender<LogLine>,
}

impl JournalState {
    fn new() -> Self {
        Self {
            buffer: VecDeque::with_capacity(RING_CAPACITY),
            initialized: false,
            manifest: None,
            writes_since_sync: 0,
        }
    }

    fn push(&mut self, line: LogLine) {
        if self.buffer.len() == RING_CAPACITY {
            self.buffer.pop_front();
        }
        self.buffer.push_back(line);
    }
}

impl Journal {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            state: AsyncMutex::new(JournalState::new()),
            tx,
        }
    }
}

pub struct InMemoryLogSink {
    processes: Mutex<HashMap<String, Arc<Journal>>>,
    runs: Mutex<HashMap<JobRunId, Arc<Journal>>>,
    known_processes: Mutex<HashSet<String>>,
    log_dir: Option<PathBuf>,
    policy: JournalPolicy,
}

impl Default for InMemoryLogSink {
    fn default() -> Self {
        Self {
            processes: Mutex::new(HashMap::new()),
            runs: Mutex::new(HashMap::new()),
            known_processes: Mutex::new(HashSet::new()),
            log_dir: None,
            policy: JournalPolicy::default(),
        }
    }
}

impl InMemoryLogSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_log_dir(log_dir: PathBuf) -> Self {
        Self::with_log_dir_and_policy(log_dir, JournalPolicy::default())
    }

    pub fn with_log_dir_and_policy(log_dir: PathBuf, policy: JournalPolicy) -> Self {
        Self {
            log_dir: Some(log_dir),
            policy,
            ..Self::default()
        }
    }

    fn process_state(&self, source: &str) -> Arc<Journal> {
        self.known_processes
            .lock()
            .unwrap()
            .insert(source.to_owned());
        let mut sources = self.processes.lock().unwrap();
        sources
            .entry(source.to_owned())
            .or_insert_with(|| Arc::new(Journal::new()))
            .clone()
    }

    fn run_state(&self, run_id: JobRunId) -> Arc<Journal> {
        let mut sources = self.runs.lock().unwrap();
        sources
            .entry(run_id)
            .or_insert_with(|| Arc::new(Journal::new()))
            .clone()
    }

    fn process_paths(&self, source: &str) -> Option<JournalPaths> {
        let dir = self.log_dir.as_ref()?;
        let key = format!("process-{}", hex_name(source));
        let safe = legacy_safe_name(source);
        let collision = self
            .known_processes
            .lock()
            .unwrap()
            .iter()
            .filter(|name| legacy_safe_name(name) == safe)
            .count()
            > 1;
        Some(JournalPaths {
            active: dir.join(format!("{key}.jsonl")),
            manifest: dir.join(format!("{key}.manifest.json")),
            segment_prefix: format!("{key}."),
            legacy: (!collision).then(|| dir.join(format!("process-{safe}.jsonl"))),
            is_run: false,
        })
    }

    fn run_paths(&self, run_id: JobRunId) -> Option<JournalPaths> {
        let dir = self.log_dir.as_ref()?;
        let key = format!("run-{}", run_id.0);
        Some(JournalPaths {
            active: dir.join(format!("{key}.jsonl")),
            manifest: dir.join(format!("{key}.manifest.json")),
            segment_prefix: format!("{key}."),
            legacy: None,
            is_run: true,
        })
    }

    async fn initialize(
        state: &mut JournalState,
        paths: Option<&JournalPaths>,
        policy: &JournalPolicy,
    ) -> Result<(), LogError> {
        if state.initialized {
            return Ok(());
        }
        let Some(paths) = paths else {
            state.manifest = Some(JournalManifest::empty(Utc::now()));
            state.initialized = true;
            return Ok(());
        };
        let parent = paths.active.parent().expect("journal path has parent");
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(storage_error)?;
        if let Some(legacy) = &paths.legacy {
            migrate_legacy_journal(&paths.active, legacy).await?;
        }
        let mut manifest = match read_manifest(&paths.manifest).await? {
            Some(manifest) => manifest,
            None => rebuild_manifest(paths).await?,
        };
        repair_manifest(paths, &mut manifest).await?;
        if !paths.is_run || manifest.sealed {
            enforce_retention(paths, &mut manifest, policy, Utc::now()).await?;
        }
        let active_lines = read_journal(&paths.active).await?.unwrap_or_default();
        state.buffer = active_lines
            .into_iter()
            .rev()
            .take(RING_CAPACITY)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        write_manifest(&paths.manifest, &manifest).await?;
        state.manifest = Some(manifest);
        state.initialized = true;
        Ok(())
    }

    async fn append_to(
        &self,
        journal: Arc<Journal>,
        paths: Option<JournalPaths>,
        mut line: LogLine,
        source_label: String,
    ) -> Result<(), LogError> {
        let mut state = journal.state.lock().await;
        Self::initialize(&mut state, paths.as_ref(), &self.policy).await?;
        let sync =
            paths.is_some() && state.writes_since_sync.saturating_add(1) >= FSYNC_EVERY_LINES;
        if paths.is_some() {
            state.writes_since_sync = if sync {
                0
            } else {
                state.writes_since_sync.saturating_add(1)
            };
        }
        let manifest = state.manifest.as_mut().expect("initialized manifest");
        if manifest.sealed {
            return Err(LogError::Sealed(source_label));
        }
        if let Some(paths) = paths.as_ref() {
            if should_rollover(manifest, &self.policy, Utc::now()) {
                rollover(paths, manifest, &self.policy).await?;
            }
        }
        line.sequence = manifest.next_sequence;
        if let Some(paths) = paths.as_ref() {
            let written = persist(&paths.active, &line, sync).await?;
            manifest.active_byte_len = manifest.active_byte_len.saturating_add(written);
        }
        manifest.high_watermark = line.sequence;
        manifest.next_sequence = line.sequence.saturating_add(1);
        // The active segment is durably synced in bounded batches and its
        // committed prefix is repaired on restart. Rewriting the manifest for
        // every line would turn high-volume output into metadata-bound I/O;
        // rollover, sealing, and retention atomically commit transitions.
        state.push(line.clone());
        let _ = journal.tx.send(line);
        Ok(())
    }

    async fn tail_from(
        &self,
        journal: Arc<Journal>,
        paths: Option<JournalPaths>,
        limit: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
        subscribe: bool,
    ) -> (LogTail, Option<broadcast::Receiver<LogLine>>) {
        let mut state = journal.state.lock().await;
        if Self::initialize(&mut state, paths.as_ref(), &self.policy)
            .await
            .is_err()
        {
            return (LogTail::default(), None);
        }
        let receiver = subscribe.then(|| journal.tx.subscribe());
        let manifest = state.manifest.as_ref().expect("initialized manifest");
        let page = match paths.as_ref() {
            Some(paths) => snapshot_durable(paths, manifest, limit, since, after_sequence)
                .await
                .unwrap_or_default(),
            None => snapshot(
                &state.buffer,
                limit,
                since,
                after_sequence,
                manifest.high_watermark,
                manifest.earliest_retained_sequence(),
            ),
        };
        (page, receiver)
    }

    async fn seal_run_to(
        &self,
        journal: Arc<Journal>,
        paths: Option<JournalPaths>,
    ) -> Result<(), LogError> {
        let mut state = journal.state.lock().await;
        Self::initialize(&mut state, paths.as_ref(), &self.policy).await?;
        let manifest = state.manifest.as_mut().expect("initialized manifest");
        manifest.sealed = true;
        if let Some(paths) = paths.as_ref() {
            tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&paths.active)
                .await
                .map_err(storage_error)?
                .sync_data()
                .await
                .map_err(storage_error)?;
            write_manifest(&paths.manifest, manifest).await?;
            enforce_retention(paths, manifest, &self.policy, Utc::now()).await?;
        }
        Ok(())
    }

    async fn remove_run_to(
        &self,
        journal: Arc<Journal>,
        paths: Option<JournalPaths>,
    ) -> Result<(), LogError> {
        self.seal_run_to(journal, paths.clone()).await?;
        let Some(paths) = paths else {
            return Ok(());
        };
        let manifest = read_manifest(&paths.manifest)
            .await?
            .unwrap_or_else(|| JournalManifest::empty(Utc::now()));
        remove_if_exists(&paths.active).await?;
        for segment in manifest.sealed_segments {
            remove_if_exists(&paths.active.with_file_name(segment.filename)).await?;
        }
        // A crash after a retention tombstone can leave an unreferenced
        // physical segment.  Run deletion is authoritative for this source,
        // so remove every matching segment idempotently as well.
        let directory = paths.active.parent().expect("journal path parent");
        let mut entries = tokio::fs::read_dir(directory)
            .await
            .map_err(storage_error)?;
        while let Some(entry) = entries.next_entry().await.map_err(storage_error)? {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.starts_with(&paths.segment_prefix)
                && name.contains("segment-")
                && name.ends_with(".jsonl")
            {
                remove_if_exists(&entry.path()).await?;
            }
        }
        remove_if_exists(&paths.manifest).await
    }
}

fn hex_name(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn legacy_safe_name(value: &str) -> String {
    value
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

fn stream_name(stream: LogStream) -> &'static str {
    match stream {
        LogStream::Stdout => "stdout",
        LogStream::Stderr => "stderr",
        LogStream::System => "system",
    }
}

fn parse_stream(value: &str) -> LogStream {
    match value {
        "stderr" => LogStream::Stderr,
        "system" => LogStream::System,
        _ => LogStream::Stdout,
    }
}

fn storage_error(error: std::io::Error) -> LogError {
    LogError::Storage(error.to_string())
}

async fn persist(path: &Path, line: &LogLine, sync: bool) -> Result<u64, LogError> {
    let encoded = serde_json::json!({
        "sequence": line.sequence,
        "timestamp": line.timestamp,
        "stream": stream_name(line.stream),
        "line": line.line,
    })
    .to_string();
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .map_err(storage_error)?;
    file.write_all(encoded.as_bytes())
        .await
        .map_err(storage_error)?;
    file.write_all(b"\n").await.map_err(storage_error)?;
    // `tokio::fs::File` may still hold completed writes in its async buffer.
    // The sequence is published immediately after this function returns, so
    // make every complete JSONL row visible to a concurrent durable reader
    // before advancing the in-memory high-watermark.  `sync_data` remains
    // batched separately to avoid making every output line a storage barrier.
    file.flush().await.map_err(storage_error)?;
    if sync {
        file.sync_data().await.map_err(storage_error)?;
    }
    Ok(encoded.len() as u64 + 1)
}

async fn read_manifest(path: &Path) -> Result<Option<JournalManifest>, LogError> {
    match tokio::fs::read(path).await {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| LogError::Storage(format!("{}: {error}", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(storage_error(error)),
    }
}

/// Persist the compact index by temp-write, flush/sync, and atomic rename.
/// Every entry is synced before this replacement, so a surviving manifest can
/// never claim a sequence that was broadcast but not recoverable from a file.
async fn write_manifest(path: &Path, manifest: &JournalManifest) -> Result<(), LogError> {
    let encoded =
        serde_json::to_vec(manifest).map_err(|error| LogError::Storage(error.to_string()))?;
    let temporary = path.with_extension(format!("manifest-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .await
        .map_err(storage_error)?;
    file.write_all(&encoded).await.map_err(storage_error)?;
    file.flush().await.map_err(storage_error)?;
    file.sync_data().await.map_err(storage_error)?;
    drop(file);
    match tokio::fs::rename(&temporary, path).await {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = tokio::fs::remove_file(&temporary).await;
            Err(storage_error(error))
        }
    }
}

async fn migrate_legacy_journal(active: &Path, legacy: &Path) -> Result<(), LogError> {
    if active == legacy
        || tokio::fs::try_exists(active).await.map_err(storage_error)?
        || !tokio::fs::try_exists(legacy).await.map_err(storage_error)?
    {
        return Ok(());
    }
    let contents = tokio::fs::read(legacy).await.map_err(storage_error)?;
    let temporary = active.with_extension(format!("jsonl.migrate-{}", uuid::Uuid::new_v4()));
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .await
        .map_err(storage_error)?;
    file.write_all(&contents).await.map_err(storage_error)?;
    file.flush().await.map_err(storage_error)?;
    file.sync_data().await.map_err(storage_error)?;
    drop(file);
    match tokio::fs::rename(&temporary, active).await {
        Ok(()) => Ok(()),
        Err(_error) if tokio::fs::try_exists(active).await.unwrap_or(false) => {
            let _ = tokio::fs::remove_file(&temporary).await;
            Ok(())
        }
        Err(error) => {
            let _ = tokio::fs::remove_file(&temporary).await;
            Err(storage_error(error))
        }
    }
}

async fn read_journal(path: &Path) -> Result<Option<VecDeque<LogLine>>, LogError> {
    let contents = match tokio::fs::read_to_string(path).await {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(storage_error(error)),
    };
    let mut next_legacy_sequence = 1;
    let mut lines = VecDeque::new();
    for encoded in contents.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(encoded) else {
            continue;
        };
        let Some(timestamp) = value
            .get("timestamp")
            .and_then(|value| value.as_str())
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc))
        else {
            continue;
        };
        let sequence = value
            .get("sequence")
            .and_then(|value| value.as_u64())
            .unwrap_or(next_legacy_sequence);
        next_legacy_sequence = next_legacy_sequence.max(sequence.saturating_add(1));
        lines.push_back(LogLine {
            sequence,
            timestamp,
            stream: value
                .get("stream")
                .and_then(|value| value.as_str())
                .map(parse_stream)
                .unwrap_or(LogStream::Stdout),
            line: value
                .get("line")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_owned(),
        });
    }
    Ok(Some(lines))
}

async fn rebuild_manifest(paths: &JournalPaths) -> Result<JournalManifest, LogError> {
    let now = Utc::now();
    let mut manifest = JournalManifest::empty(now);
    let active = read_journal(&paths.active).await?.unwrap_or_default();
    if let Some(first) = active.front() {
        manifest.active_start_sequence = first.sequence;
        manifest.active_created_at = first.timestamp;
    }
    if let Some(last) = active.back() {
        manifest.high_watermark = last.sequence;
        manifest.next_sequence = last.sequence.saturating_add(1);
    }
    manifest.active_byte_len = tokio::fs::metadata(&paths.active)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    Ok(manifest)
}

/// Recovery only inspects the compact manifest, the bounded active segment,
/// and orphan segment names created during an interrupted rollover.
async fn repair_manifest(
    paths: &JournalPaths,
    manifest: &mut JournalManifest,
) -> Result<(), LogError> {
    for filename in manifest.deleted_segments.clone() {
        remove_if_exists(&paths.active.with_file_name(filename)).await?;
    }
    manifest.deleted_segments.clear();
    let active = read_journal(&paths.active).await?.unwrap_or_default();
    if let Some(first) = active.front() {
        manifest.active_start_sequence = first.sequence;
    }
    if let Some(last) = active.back() {
        manifest.high_watermark = manifest.high_watermark.max(last.sequence);
        manifest.next_sequence = manifest.next_sequence.max(last.sequence.saturating_add(1));
    }
    manifest.active_byte_len = tokio::fs::metadata(&paths.active)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let directory = paths.active.parent().expect("journal path parent");
    let mut entries = tokio::fs::read_dir(directory)
        .await
        .map_err(storage_error)?;
    while let Some(entry) = entries.next_entry().await.map_err(storage_error)? {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(value) = name
            .strip_prefix(&paths.segment_prefix)
            .and_then(|value| value.strip_prefix("segment-"))
            .and_then(|value| value.strip_suffix(".jsonl"))
        else {
            continue;
        };
        let Ok(start) = value.parse::<u64>() else {
            continue;
        };
        if manifest
            .sealed_segments
            .iter()
            .any(|segment| segment.filename == name)
        {
            continue;
        }
        let lines = read_journal(&entry.path()).await?.unwrap_or_default();
        let Some(last) = lines.back() else {
            continue;
        };
        manifest.sealed_segments.push(SegmentMeta {
            filename: name,
            first_sequence: lines.front().map(|line| line.sequence).unwrap_or(start),
            last_sequence: last.sequence,
            created_at: lines
                .front()
                .map(|line| line.timestamp)
                .unwrap_or_else(Utc::now),
            sealed_at: Utc::now(),
            byte_len: entry.metadata().await.map_err(storage_error)?.len(),
        });
        manifest.high_watermark = manifest.high_watermark.max(last.sequence);
        manifest.next_sequence = manifest.next_sequence.max(last.sequence.saturating_add(1));
    }
    manifest
        .sealed_segments
        .sort_by_key(|segment| segment.first_sequence);
    if !tokio::fs::try_exists(&paths.active)
        .await
        .map_err(storage_error)?
    {
        manifest.active_start_sequence = manifest.next_sequence;
        manifest.active_created_at = Utc::now();
        manifest.active_byte_len = 0;
    }
    manifest.version = MANIFEST_VERSION;
    Ok(())
}

fn should_rollover(manifest: &JournalManifest, policy: &JournalPolicy, now: DateTime<Utc>) -> bool {
    manifest.active_byte_len > 0
        && (manifest.active_byte_len >= policy.max_segment_bytes
            || now - manifest.active_created_at >= policy.max_segment_age)
}

async fn rollover(
    paths: &JournalPaths,
    manifest: &mut JournalManifest,
    policy: &JournalPolicy,
) -> Result<(), LogError> {
    let now = Utc::now();
    let old_start = manifest.active_start_sequence;
    let sealed_path = paths.segment(old_start);
    // Flush every previously completed append before deriving the sealed
    // segment range. Otherwise the in-memory high-watermark can advance past
    // rows that a concurrent file descriptor has not made visible yet.
    tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.active)
        .await
        .map_err(storage_error)?
        .sync_data()
        .await
        .map_err(storage_error)?;
    let active_lines = read_journal(&paths.active).await?.unwrap_or_default();
    if let Some(last) = active_lines.back() {
        tokio::fs::rename(&paths.active, &sealed_path)
            .await
            .map_err(storage_error)?;
        manifest.sealed_segments.push(SegmentMeta {
            filename: sealed_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned(),
            first_sequence: active_lines
                .front()
                .map(|line| line.sequence)
                .unwrap_or(old_start),
            last_sequence: last.sequence,
            created_at: manifest.active_created_at,
            sealed_at: now,
            byte_len: manifest.active_byte_len,
        });
    }
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&paths.active)
        .await
        .map_err(storage_error)?;
    file.sync_data().await.map_err(storage_error)?;
    manifest.active_start_sequence = manifest.next_sequence;
    manifest.active_created_at = now;
    manifest.active_byte_len = 0;
    if !paths.is_run || manifest.sealed {
        enforce_retention(paths, manifest, policy, now).await?;
    }
    write_manifest(&paths.manifest, manifest).await
}

async fn enforce_retention(
    paths: &JournalPaths,
    manifest: &mut JournalManifest,
    policy: &JournalPolicy,
    now: DateTime<Utc>,
) -> Result<(), LogError> {
    let mut total_bytes = manifest.active_byte_len.saturating_add(
        manifest
            .sealed_segments
            .iter()
            .map(|segment| segment.byte_len)
            .sum::<u64>(),
    );
    while let Some(first) = manifest.sealed_segments.first() {
        let too_old = now - first.sealed_at >= policy.max_segment_age;
        let too_many = manifest.sealed_segments.len() > policy.max_sealed_segments;
        let too_large = total_bytes > policy.max_total_bytes;
        if !too_old && !too_many && !too_large {
            break;
        }
        let removed = manifest.sealed_segments.remove(0);
        // Commit the tombstone before physical deletion; an interrupted delete
        // is safely retried as an unreferenced file on later cleanup.
        manifest.deleted_segments.push(removed.filename.clone());
        write_manifest(&paths.manifest, manifest).await?;
        remove_if_exists(&paths.active.with_file_name(&removed.filename)).await?;
        manifest
            .deleted_segments
            .retain(|filename| filename != &removed.filename);
        write_manifest(&paths.manifest, manifest).await?;
        total_bytes = total_bytes.saturating_sub(removed.byte_len);
    }
    Ok(())
}

async fn remove_if_exists(path: &Path) -> Result<(), LogError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_error(error)),
    }
}

async fn snapshot_durable(
    paths: &JournalPaths,
    manifest: &JournalManifest,
    limit: usize,
    since: Option<DateTime<Utc>>,
    after_sequence: Option<u64>,
) -> Result<LogTail, LogError> {
    let mut lines = VecDeque::new();
    for segment in &manifest.sealed_segments {
        if after_sequence.is_some_and(|cursor| segment.last_sequence <= cursor) {
            continue;
        }
        if let Some(mut segment_lines) =
            read_journal(&paths.active.with_file_name(&segment.filename)).await?
        {
            lines.append(&mut segment_lines);
        }
    }
    if after_sequence.is_none_or(|cursor| manifest.high_watermark > cursor) {
        if let Some(mut active_lines) = read_journal(&paths.active).await? {
            lines.append(&mut active_lines);
        }
    }
    Ok(snapshot(
        &lines,
        limit,
        since,
        after_sequence,
        manifest.high_watermark,
        manifest.earliest_retained_sequence(),
    ))
}

fn snapshot(
    buffer: &VecDeque<LogLine>,
    limit: usize,
    since: Option<DateTime<Utc>>,
    after_sequence: Option<u64>,
    high_watermark: u64,
    earliest_retained_sequence: Option<u64>,
) -> LogTail {
    let cursor_expired = after_sequence.is_some_and(|cursor| {
        earliest_retained_sequence.is_some_and(|earliest| cursor.saturating_add(1) < earliest)
    });
    let filtered: Vec<LogLine> = buffer
        .iter()
        .filter(|line| since.is_none_or(|value| line.timestamp >= value))
        .filter(|line| after_sequence.is_none_or(|value| line.sequence > value))
        .cloned()
        .collect();
    let truncated = limit > 0 && filtered.len() > limit;
    let lines = if truncated {
        filtered[filtered.len() - limit..].to_vec()
    } else {
        filtered
    };
    LogTail {
        lines,
        truncated,
        high_watermark,
        next_sequence: high_watermark.saturating_add(1),
        earliest_retained_sequence,
        cursor_expired,
    }
}

#[async_trait]
impl LogSink for InMemoryLogSink {
    fn register_process_names(&self, names: &[String]) {
        self.known_processes
            .lock()
            .unwrap()
            .extend(names.iter().cloned());
    }

    async fn append(&self, source: &str, line: LogLine) -> Result<(), LogError> {
        self.append_to(
            self.process_state(source),
            self.process_paths(source),
            line,
            source.to_owned(),
        )
        .await
    }

    async fn tail(
        &self,
        source: &str,
        limit: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
    ) -> LogTail {
        self.tail_from(
            self.process_state(source),
            self.process_paths(source),
            limit,
            since,
            after_sequence,
            false,
        )
        .await
        .0
    }

    fn subscribe(&self, source: &str) -> broadcast::Receiver<LogLine> {
        self.process_state(source).tx.subscribe()
    }

    async fn subscribe_tail(
        &self,
        source: &str,
        limit: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
    ) -> (LogTail, broadcast::Receiver<LogLine>) {
        let (page, receiver) = self
            .tail_from(
                self.process_state(source),
                self.process_paths(source),
                limit,
                since,
                after_sequence,
                true,
            )
            .await;
        (page, receiver.expect("receiver is requested"))
    }

    async fn append_run(&self, run_id: JobRunId, line: LogLine) -> Result<(), LogError> {
        self.append_to(
            self.run_state(run_id),
            self.run_paths(run_id),
            line,
            run_id.0.to_string(),
        )
        .await
    }

    async fn tail_run(
        &self,
        run_id: JobRunId,
        limit: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
    ) -> LogTail {
        self.tail_from(
            self.run_state(run_id),
            self.run_paths(run_id),
            limit,
            since,
            after_sequence,
            false,
        )
        .await
        .0
    }

    async fn seal_run(&self, run_id: JobRunId) -> Result<(), LogError> {
        self.seal_run_to(self.run_state(run_id), self.run_paths(run_id))
            .await
    }

    async fn remove_run(&self, run_id: JobRunId) -> Result<(), LogError> {
        self.remove_run_to(self.run_state(run_id), self.run_paths(run_id))
            .await
    }

    fn subscribe_run(&self, run_id: JobRunId) -> broadcast::Receiver<LogLine> {
        self.run_state(run_id).tx.subscribe()
    }

    async fn subscribe_tail_run(
        &self,
        run_id: JobRunId,
        limit: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
    ) -> (LogTail, broadcast::Receiver<LogLine>) {
        let (page, receiver) = self
            .tail_from(
                self.run_state(run_id),
                self.run_paths(run_id),
                limit,
                since,
                after_sequence,
                true,
            )
            .await;
        (page, receiver.expect("receiver is requested"))
    }

    async fn persisted_run_ids(&self) -> Result<Vec<JobRunId>, LogError> {
        let Some(directory) = &self.log_dir else {
            return Ok(Vec::new());
        };
        let mut entries = match tokio::fs::read_dir(directory).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(storage_error(error)),
        };
        let mut run_ids = HashSet::new();
        while let Some(entry) = entries.next_entry().await.map_err(storage_error)? {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let value = name
                .strip_prefix("run-")
                .and_then(|name| name.strip_suffix(".manifest.json"))
                .or_else(|| {
                    name.strip_prefix("run-")
                        .and_then(|name| name.strip_suffix(".jsonl"))
                });
            if let Some(value) = value {
                if let Ok(id) = uuid::Uuid::parse_str(value) {
                    run_ids.insert(JobRunId(id));
                }
            }
        }
        Ok(run_ids.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::InMemoryLogSink;
    use my_supervisor_core::domain::{JobRunId, LogLine, LogStream};
    use my_supervisor_core::ports::LogSink;

    fn log_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("my-supervisor-journal-{}", uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn sequences_are_durable_monotonic_and_seal_prevents_recreation() {
        let directory = log_dir();
        tokio::fs::create_dir_all(&directory).await.unwrap();
        let run_id = JobRunId::new();
        let sink = InMemoryLogSink::with_log_dir(directory.clone());
        for _ in 0..10_001 {
            sink.append_run(run_id, LogLine::now(LogStream::Stdout, "same"))
                .await
                .unwrap();
        }
        let page = sink.tail_run(run_id, 10_001, None, None).await;
        assert_eq!(page.high_watermark, 10_001);
        assert_eq!(page.lines.len(), 10_001);
        assert_eq!(page.lines.first().unwrap().sequence, 1);
        sink.seal_run(run_id).await.unwrap();
        assert!(sink
            .append_run(run_id, LogLine::now(LogStream::Stdout, "late"))
            .await
            .is_err());
        drop(sink);
        let restarted = InMemoryLogSink::with_log_dir(directory.clone());
        let page = restarted.tail_run(run_id, 0, None, None).await;
        assert_eq!(page.high_watermark, 10_001);
        tokio::fs::remove_dir_all(directory).await.unwrap();
    }

    #[tokio::test]
    async fn subscribe_snapshot_boundary_has_no_duplicate_sequences() {
        let sink = InMemoryLogSink::new();
        sink.append("process", LogLine::now(LogStream::Stdout, "before"))
            .await
            .unwrap();
        let (page, mut receiver) = sink.subscribe_tail("process", 100, None, None).await;
        sink.append("process", LogLine::now(LogStream::Stderr, "after"))
            .await
            .unwrap();
        let live = receiver.recv().await.unwrap();
        assert_eq!(page.high_watermark, 1);
        assert_eq!(page.lines[0].sequence, 1);
        assert_eq!(live.sequence, 2);
    }
}
