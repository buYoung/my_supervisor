//! `my-supervisor-infra-logging` — in-memory `LogSink`. Keeps a bounded ring
//! buffer per source and per job run, plus a live broadcast channel. Rotation /
//! backpressure / on-disk run archives are deferred (brief: out of scope).

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::broadcast;

use my_supervisor_core::domain::{JobRunId, LogLine};
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
}

impl InMemoryLogSink {
    pub fn new() -> Self {
        Self::default()
    }
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
        let mut map = self.processes.lock().unwrap();
        map.entry(source.to_string())
            .or_insert_with(Channel::new)
            .push(line);
    }

    async fn tail(&self, source: &str, limit: usize, since: Option<DateTime<Utc>>) -> LogTail {
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
        let mut map = self.runs.lock().unwrap();
        map.entry(run_id).or_insert_with(Channel::new).push(line);
    }

    async fn tail_run(&self, run_id: JobRunId, limit: usize) -> Vec<LogLine> {
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
