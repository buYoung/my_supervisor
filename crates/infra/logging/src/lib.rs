//! Durable, source-local JSONL journals for process and job-run output.
//!
//! A source lock owns sequence allocation, disk append, ring insertion and
//! broadcast.  This ordering is intentional: an observer can only receive a
//! cursor that is already recoverable from the journal.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, Mutex as AsyncMutex};

use my_supervisor_core::domain::{JobRunId, LogLine, LogStream};
use my_supervisor_core::ports::{LogError, LogSink, LogTail};

const RING_CAPACITY: usize = 10_000;
const BROADCAST_CAPACITY: usize = 10_256;
const FSYNC_EVERY_LINES: u64 = 64;

struct JournalState {
    buffer: VecDeque<LogLine>,
    next_sequence: u64,
    initialized: bool,
    sealed: bool,
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
            next_sequence: 1,
            initialized: false,
            sealed: false,
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
        Self { state: AsyncMutex::new(JournalState::new()), tx }
    }
}

#[derive(Default)]
pub struct InMemoryLogSink {
    processes: Mutex<HashMap<String, Arc<Journal>>>,
    runs: Mutex<HashMap<JobRunId, Arc<Journal>>>,
    /// Names loaded at bootstrap are used to decide whether a historic
    /// sanitized filename is a singleton or an ambiguous collision.
    known_processes: Mutex<HashSet<String>>,
    log_dir: Option<PathBuf>,
}

impl InMemoryLogSink {
    pub fn new() -> Self { Self::default() }

    pub fn with_log_dir(log_dir: PathBuf) -> Self {
        Self { log_dir: Some(log_dir), ..Self::default() }
    }

    fn process_state(&self, source: &str) -> Arc<Journal> {
        self.known_processes.lock().unwrap().insert(source.to_owned());
        let mut sources = self.processes.lock().unwrap();
        sources.entry(source.to_owned()).or_insert_with(|| Arc::new(Journal::new())).clone()
    }

    fn run_state(&self, run_id: JobRunId) -> Arc<Journal> {
        let mut sources = self.runs.lock().unwrap();
        sources.entry(run_id).or_insert_with(|| Arc::new(Journal::new())).clone()
    }

    fn process_path(&self, source: &str) -> Option<PathBuf> {
        self.log_dir.as_ref().map(|dir| dir.join(format!("process-{}.jsonl", hex_name(source))))
    }

    fn legacy_process_path(&self, source: &str) -> Option<PathBuf> {
        let safe = legacy_safe_name(source);
        let collision = self.known_processes.lock().unwrap().iter()
            .filter(|name| legacy_safe_name(name) == safe).count() > 1;
        (!collision).then(|| self.log_dir.as_ref().map(|dir| dir.join(format!("process-{safe}.jsonl")))).flatten()
    }

    fn run_path(&self, run_id: JobRunId) -> Option<PathBuf> {
        self.log_dir.as_ref().map(|dir| dir.join(format!("run-{}.jsonl", run_id.0)))
    }

    async fn initialize(state: &mut JournalState, path: Option<&Path>, legacy: Option<&Path>) -> Result<(), LogError> {
        if state.initialized { return Ok(()); }
        if let (Some(path), Some(legacy)) = (path, legacy) {
            migrate_legacy_journal(path, legacy).await?;
        }
        let lines = if let Some(path) = path {
            match read_journal(path).await? {
                Some(lines) => lines,
                None if let Some(legacy) = legacy => read_journal(legacy).await?.unwrap_or_default(),
                None => VecDeque::new(),
            }
        } else { VecDeque::new() };
        let high_watermark = lines.back().map(|line| line.sequence).unwrap_or(0);
        state.buffer = lines.into_iter().rev().take(RING_CAPACITY).collect::<Vec<_>>().into_iter().rev().collect();
        state.next_sequence = high_watermark.saturating_add(1).max(1);
        state.initialized = true;
        Ok(())
    }

    async fn append_to(
        journal: Arc<Journal>,
        path: Option<PathBuf>,
        legacy: Option<PathBuf>,
        mut line: LogLine,
        source_label: String,
    ) -> Result<(), LogError> {
        let mut state = journal.state.lock().await;
        Self::initialize(&mut state, path.as_deref(), legacy.as_deref()).await?;
        if state.sealed { return Err(LogError::Sealed(source_label)); }
        line.sequence = state.next_sequence;
        state.writes_since_sync = state.writes_since_sync.saturating_add(1);
        if let Some(path) = path.as_deref() { persist(path, &line, state.writes_since_sync >= FSYNC_EVERY_LINES).await?; }
        if state.writes_since_sync >= FSYNC_EVERY_LINES { state.writes_since_sync = 0; }
        state.next_sequence = state.next_sequence.saturating_add(1);
        state.push(line.clone());
        let _ = journal.tx.send(line);
        Ok(())
    }

    async fn tail_from(
        journal: Arc<Journal>,
        path: Option<PathBuf>,
        legacy: Option<PathBuf>,
        limit: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
        subscribe: bool,
    ) -> (LogTail, Option<broadcast::Receiver<LogLine>>) {
        let mut state = journal.state.lock().await;
        // A read failure is represented as an empty page because LogSink tail
        // is a read API.  Append/delete paths surface the same error to the
        // durable cleanup owner instead of claiming success.
        let _ = Self::initialize(&mut state, path.as_deref(), legacy.as_deref()).await;
        let receiver = subscribe.then(|| journal.tx.subscribe());
        // The in-memory ring only accelerates recent tails.  A reconnect can
        // legitimately resume before that ring, so recover from the durable
        // journal before declaring a cursor complete.
        let needs_durable_recovery = path.is_some() && (
            since.is_some()
                || limit == 0
                || limit > state.buffer.len()
                || after_sequence.is_some_and(|sequence| {
                    state.buffer.front().is_some_and(|first| sequence < first.sequence.saturating_sub(1))
                })
        );
        let recovered = if needs_durable_recovery {
            match path.as_deref() {
                Some(path) => read_journal(path).await.ok().flatten(),
                None => None,
            }
        } else {
            None
        };
        let lines = recovered.as_ref().unwrap_or(&state.buffer);
        let page = snapshot(lines, limit, since, after_sequence, state.next_sequence.saturating_sub(1));
        (page, receiver)
    }
}

fn hex_name(value: &str) -> String {
    value.as_bytes().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn legacy_safe_name(value: &str) -> String {
    value.chars().map(|character| {
        if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') { character } else { '_' }
    }).collect()
}

fn stream_name(stream: LogStream) -> &'static str {
    match stream { LogStream::Stdout => "stdout", LogStream::Stderr => "stderr", LogStream::System => "system" }
}

fn parse_stream(value: &str) -> LogStream {
    match value { "stderr" => LogStream::Stderr, "system" => LogStream::System, _ => LogStream::Stdout }
}

async fn persist(path: &Path, line: &LogLine, sync: bool) -> Result<(), LogError> {
    let encoded = serde_json::json!({
        "sequence": line.sequence,
        "timestamp": line.timestamp,
        "stream": stream_name(line.stream),
        "line": line.line,
    }).to_string();
    let mut file = tokio::fs::OpenOptions::new().create(true).append(true).open(path).await
        .map_err(|error| LogError::Storage(format!("{}: {error}", path.display())))?;
    file.write_all(encoded.as_bytes()).await.map_err(|error| LogError::Storage(error.to_string()))?;
    file.write_all(b"\n").await.map_err(|error| LogError::Storage(error.to_string()))?;
    file.flush().await.map_err(|error| LogError::Storage(error.to_string()))?;
    if sync { file.sync_data().await.map_err(|error| LogError::Storage(error.to_string()))?; }
    Ok(())
}

/// Copies a singleton legacy JSONL file to its hex-keyed journal before the
/// first append.  The destination file itself is the durable migration marker;
/// a temporary sibling is synced and atomically renamed so a restart sees
/// either the intact legacy source or the complete new journal, never a
/// partially copied history.
async fn migrate_legacy_journal(path: &Path, legacy: &Path) -> Result<(), LogError> {
    if tokio::fs::try_exists(path).await.map_err(|error| LogError::Storage(error.to_string()))?
        || !tokio::fs::try_exists(legacy).await.map_err(|error| LogError::Storage(error.to_string()))?
    {
        return Ok(());
    }
    let contents = tokio::fs::read(legacy)
        .await
        .map_err(|error| LogError::Storage(format!("{}: {error}", legacy.display())))?;
    let temporary = path.with_extension(format!("jsonl.migrate-{}", uuid::Uuid::new_v4()));
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .await
        .map_err(|error| LogError::Storage(format!("{}: {error}", temporary.display())))?;
    file.write_all(&contents).await.map_err(|error| LogError::Storage(error.to_string()))?;
    file.flush().await.map_err(|error| LogError::Storage(error.to_string()))?;
    file.sync_data().await.map_err(|error| LogError::Storage(error.to_string()))?;
    drop(file);
    match tokio::fs::rename(&temporary, path).await {
        Ok(()) => Ok(()),
        Err(_error) if tokio::fs::try_exists(path).await.unwrap_or(false) => {
            let _ = tokio::fs::remove_file(&temporary).await;
            Ok(())
        }
        Err(error) => {
            let _ = tokio::fs::remove_file(&temporary).await;
            Err(LogError::Storage(format!("{}: {error}", path.display())))
        }
    }
}

async fn read_journal(path: &Path) -> Result<Option<VecDeque<LogLine>>, LogError> {
    let contents = match tokio::fs::read_to_string(path).await {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(LogError::Storage(format!("{}: {error}", path.display()))),
    };
    let mut next_legacy_sequence = 1;
    let mut lines = VecDeque::new();
    for encoded in contents.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(encoded) else { continue; };
        let Some(timestamp) = value.get("timestamp").and_then(|value| value.as_str())
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok()).map(|value| value.with_timezone(&Utc)) else { continue; };
        let sequence = value.get("sequence").and_then(|value| value.as_u64()).unwrap_or(next_legacy_sequence);
        next_legacy_sequence = next_legacy_sequence.max(sequence.saturating_add(1));
        lines.push_back(LogLine {
            sequence,
            timestamp,
            stream: value.get("stream").and_then(|value| value.as_str()).map(parse_stream).unwrap_or(LogStream::Stdout),
            line: value.get("line").and_then(|value| value.as_str()).unwrap_or_default().to_owned(),
        });
    }
    Ok(Some(lines))
}

fn snapshot(
    buffer: &VecDeque<LogLine>, limit: usize, since: Option<DateTime<Utc>>, after_sequence: Option<u64>, high_watermark: u64,
) -> LogTail {
    let filtered: Vec<LogLine> = buffer.iter()
        .filter(|line| since.is_none_or(|value| line.timestamp >= value))
        .filter(|line| after_sequence.is_none_or(|value| line.sequence > value))
        .cloned().collect();
    let truncated = limit > 0 && filtered.len() > limit;
    let lines = if truncated { filtered[filtered.len() - limit..].to_vec() } else { filtered };
    LogTail { lines, truncated, high_watermark, next_sequence: high_watermark.saturating_add(1) }
}

#[async_trait]
impl LogSink for InMemoryLogSink {
    fn register_process_names(&self, names: &[String]) {
        self.known_processes.lock().unwrap().extend(names.iter().cloned());
    }

    async fn append(&self, source: &str, line: LogLine) -> Result<(), LogError> {
        Self::append_to(self.process_state(source), self.process_path(source), self.legacy_process_path(source), line, source.to_owned()).await
    }

    async fn tail(&self, source: &str, limit: usize, since: Option<DateTime<Utc>>, after_sequence: Option<u64>) -> LogTail {
        Self::tail_from(self.process_state(source), self.process_path(source), self.legacy_process_path(source), limit, since, after_sequence, false).await.0
    }

    fn subscribe(&self, source: &str) -> broadcast::Receiver<LogLine> {
        self.process_state(source).tx.subscribe()
    }

    async fn subscribe_tail(&self, source: &str, limit: usize, since: Option<DateTime<Utc>>, after_sequence: Option<u64>) -> (LogTail, broadcast::Receiver<LogLine>) {
        let (page, receiver) = Self::tail_from(self.process_state(source), self.process_path(source), self.legacy_process_path(source), limit, since, after_sequence, true).await;
        (page, receiver.expect("receiver is requested"))
    }

    async fn append_run(&self, run_id: JobRunId, line: LogLine) -> Result<(), LogError> {
        Self::append_to(self.run_state(run_id), self.run_path(run_id), None, line, run_id.0.to_string()).await
    }

    async fn tail_run(&self, run_id: JobRunId, limit: usize, since: Option<DateTime<Utc>>, after_sequence: Option<u64>) -> LogTail {
        Self::tail_from(self.run_state(run_id), self.run_path(run_id), None, limit, since, after_sequence, false).await.0
    }

    async fn seal_run(&self, run_id: JobRunId) -> Result<(), LogError> {
        let journal = self.run_state(run_id);
        let mut state = journal.state.lock().await;
        Self::initialize(&mut state, self.run_path(run_id).as_deref(), None).await?;
        if let Some(path) = self.run_path(run_id) {
            // A successful no-output Run still owns a durable, sealed journal.
            // Creating the empty file here keeps terminal persistence separate
            // from whether either pump happened to emit a line.
            let file = tokio::fs::OpenOptions::new().create(true).append(true).open(path).await
                .map_err(|error| LogError::Storage(error.to_string()))?;
            file.sync_data().await.map_err(|error| LogError::Storage(error.to_string()))?;
        }
        state.sealed = true;
        Ok(())
    }

    async fn remove_run(&self, run_id: JobRunId) -> Result<(), LogError> {
        self.seal_run(run_id).await?;
        if let Some(path) = self.run_path(run_id) {
            match tokio::fs::remove_file(&path).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(LogError::Storage(format!("{}: {error}", path.display()))),
            }
        } else { Ok(()) }
    }

    fn subscribe_run(&self, run_id: JobRunId) -> broadcast::Receiver<LogLine> {
        self.run_state(run_id).tx.subscribe()
    }

    async fn subscribe_tail_run(&self, run_id: JobRunId, limit: usize, since: Option<DateTime<Utc>>, after_sequence: Option<u64>) -> (LogTail, broadcast::Receiver<LogLine>) {
        let (page, receiver) = Self::tail_from(self.run_state(run_id), self.run_path(run_id), None, limit, since, after_sequence, true).await;
        (page, receiver.expect("receiver is requested"))
    }

    async fn persisted_run_ids(&self) -> Result<Vec<JobRunId>, LogError> {
        let Some(directory) = &self.log_dir else { return Ok(Vec::new()); };
        let mut entries = tokio::fs::read_dir(directory).await
            .map_err(|error| LogError::Storage(format!("{}: {error}", directory.display())))?;
        let mut run_ids = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|error| LogError::Storage(error.to_string()))? {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue; };
            let Some(value) = name.strip_prefix("run-").and_then(|name| name.strip_suffix(".jsonl")) else { continue; };
            if let Ok(id) = uuid::Uuid::parse_str(value) { run_ids.push(JobRunId(id)); }
        }
        Ok(run_ids)
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
            sink.append_run(run_id, LogLine::now(LogStream::Stdout, "same")).await.unwrap();
        }
        let page = sink.tail_run(run_id, 10_001, None, None).await;
        assert_eq!(page.high_watermark, 10_001);
        assert_eq!(page.lines.len(), 10_001);
        assert_eq!(page.lines.first().unwrap().sequence, 1);
        sink.seal_run(run_id).await.unwrap();
        assert!(sink.append_run(run_id, LogLine::now(LogStream::Stdout, "late")).await.is_err());
        drop(sink);
        let restarted = InMemoryLogSink::with_log_dir(directory.clone());
        let page = restarted.tail_run(run_id, 0, None, None).await;
        assert_eq!(page.high_watermark, 10_001);
        tokio::fs::remove_dir_all(directory).await.unwrap();
    }

    #[tokio::test]
    async fn subscribe_snapshot_boundary_has_no_duplicate_sequences() {
        let sink = InMemoryLogSink::new();
        sink.append("process", LogLine::now(LogStream::Stdout, "before")).await.unwrap();
        let (page, mut receiver) = sink.subscribe_tail("process", 100, None, None).await;
        sink.append("process", LogLine::now(LogStream::Stderr, "after")).await.unwrap();
        let live = receiver.recv().await.unwrap();
        assert_eq!(page.high_watermark, 1);
        assert_eq!(page.lines[0].sequence, 1);
        assert_eq!(live.sequence, 2);
    }
}
