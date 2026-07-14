//! Bounded live log channels backed by JSON-lines files when a log directory is
//! configured. Disk files preserve process and job-run output across restarts.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::broadcast;
use tokio::io::AsyncWriteExt;

use my_supervisor_core::domain::{JobRunId, LogLine, LogStream};
use my_supervisor_core::ports::log_sink::LogTail;
use my_supervisor_core::ports::LogSink;

const RING_CAPACITY: usize = 2000;
const BROADCAST_CAPACITY: usize = 256;

struct Channel {
    buffer: VecDeque<LogLine>,
    tx: broadcast::Sender<LogLine>,
}

impl Channel {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Channel {
            buffer: VecDeque::with_capacity(RING_CAPACITY),
            tx,
        }
    }

    fn push(&mut self, line: LogLine) {
        if self.buffer.len() == RING_CAPACITY {
            self.buffer.pop_front();
        }
        self.buffer.push_back(line.clone());
        let _ = self.tx.send(line);
    }
}

#[derive(Default)]
pub struct InMemoryLogSink {
    processes: Mutex<HashMap<String, Channel>>,
    runs: Mutex<HashMap<JobRunId, Channel>>,
    log_dir: Option<PathBuf>,
}

impl InMemoryLogSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_log_dir(log_dir: PathBuf) -> Self {
        InMemoryLogSink {
            log_dir: Some(log_dir),
            ..Self::default()
        }
    }

    fn process_path(&self, source: &str) -> Option<PathBuf> {
        self.log_dir
            .as_ref()
            .map(|dir| dir.join(format!("process-{}.jsonl", safe_name(source))))
    }

    fn run_path(&self, run_id: JobRunId) -> Option<PathBuf> {
        self.log_dir
            .as_ref()
            .map(|dir| dir.join(format!("run-{}.jsonl", run_id.0)))
    }
}

fn safe_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
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

async fn persist(path: &Path, line: &LogLine) {
    let encoded = serde_json::json!({
        "timestamp": line.timestamp,
        "stream": stream_name(line.stream),
        "line": line.line,
    })
    .to_string();
    if let Ok(mut file) = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
    {
        let _ = file.write_all(format!("{encoded}\n").as_bytes()).await;
    }
}

async fn read_persisted(path: &Path) -> VecDeque<LogLine> {
    let Ok(contents) = tokio::fs::read_to_string(path).await else {
        return VecDeque::new();
    };
    contents
        .lines()
        .filter_map(|encoded| {
            let value: serde_json::Value = serde_json::from_str(encoded).ok()?;
            let timestamp = DateTime::parse_from_rfc3339(value.get("timestamp")?.as_str()?)
                .ok()?
                .with_timezone(&Utc);
            Some(LogLine {
                timestamp,
                stream: parse_stream(value.get("stream")?.as_str()?),
                line: value.get("line")?.as_str()?.to_string(),
            })
        })
        .collect()
}

fn snapshot(buffer: &VecDeque<LogLine>, limit: usize, since: Option<DateTime<Utc>>) -> LogTail {
    let collected: Vec<LogLine> = buffer
        .iter()
        .filter(|l| since.is_none_or(|s| l.timestamp >= s))
        .cloned()
        .collect();
    if limit == 0 || collected.len() <= limit {
        LogTail {
            lines: collected,
            truncated: false,
        }
    } else {
        LogTail {
            lines: collected[collected.len() - limit..].to_vec(),
            truncated: true,
        }
    }
}

#[async_trait]
impl LogSink for InMemoryLogSink {
    async fn append(&self, source: &str, line: LogLine) {
        if let Some(path) = self.process_path(source) {
            persist(&path, &line).await;
        }
        let mut map = self.processes.lock().unwrap();
        map.entry(source.to_string())
            .or_insert_with(Channel::new)
            .push(line);
    }

    async fn tail(&self, source: &str, limit: usize, since: Option<DateTime<Utc>>) -> LogTail {
        if let Some(path) = self.process_path(source) {
            let persisted = read_persisted(&path).await;
            return snapshot(&persisted, limit, since);
        }
        let map = self.processes.lock().unwrap();
        map.get(source)
            .map(|c| snapshot(&c.buffer, limit, since))
            .unwrap_or_default()
    }

    fn subscribe(&self, source: &str) -> broadcast::Receiver<LogLine> {
        let mut map = self.processes.lock().unwrap();
        map.entry(source.to_string())
            .or_insert_with(Channel::new)
            .tx
            .subscribe()
    }

    async fn append_run(&self, run_id: JobRunId, line: LogLine) {
        if let Some(path) = self.run_path(run_id) {
            persist(&path, &line).await;
        }
        let mut map = self.runs.lock().unwrap();
        map.entry(run_id).or_insert_with(Channel::new).push(line);
    }

    async fn tail_run(&self, run_id: JobRunId, limit: usize) -> Vec<LogLine> {
        if let Some(path) = self.run_path(run_id) {
            let persisted = read_persisted(&path).await;
            return snapshot(&persisted, limit, None).lines;
        }
        let map = self.runs.lock().unwrap();
        map.get(&run_id)
            .map(|c| snapshot(&c.buffer, limit, None).lines)
            .unwrap_or_default()
    }

    fn subscribe_run(&self, run_id: JobRunId) -> broadcast::Receiver<LogLine> {
        let mut map = self.runs.lock().unwrap();
        map.entry(run_id)
            .or_insert_with(Channel::new)
            .tx
            .subscribe()
    }
}
