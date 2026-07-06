//! `LogSink` — collects stdout/stderr lines from managed children and job runs,
//! offering tail reads and live subscription.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::broadcast;

use crate::domain::{JobRunId, LogLine};

/// Result of a tail read: the returned lines plus whether older matching lines
/// were dropped by the `limit`.
#[derive(Debug, Clone, Default)]
pub struct LogTail {
    pub lines: Vec<LogLine>,
    pub truncated: bool,
}

#[async_trait]
pub trait LogSink: Send + Sync {
    /// Append a captured line for a managed process (keyed by process name).
    async fn append(&self, source: &str, line: LogLine);
    /// Return up to `limit` recent lines, optionally filtered to `since`, and
    /// whether older matching lines were truncated by the limit.
    async fn tail(&self, source: &str, limit: usize, since: Option<DateTime<Utc>>) -> LogTail;
    /// Live subscription to a process's new lines.
    fn subscribe(&self, source: &str) -> broadcast::Receiver<LogLine>;

    /// Append a captured line for a specific job run.
    async fn append_run(&self, run_id: JobRunId, line: LogLine);
    /// Return up to `limit` recent lines for a job run.
    async fn tail_run(&self, run_id: JobRunId, limit: usize) -> Vec<LogLine>;
    /// Live subscription to a job run's new lines.
    fn subscribe_run(&self, run_id: JobRunId) -> broadcast::Receiver<LogLine>;
}
