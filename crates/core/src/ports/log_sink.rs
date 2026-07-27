//! `LogSink` — collects stdout/stderr lines from managed children and job runs,
//! offering tail reads and live subscription.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::broadcast;

use crate::domain::{JobRunId, LogLine};
use crate::ports::error::LogError;

/// Result of a tail read: returned lines, page-limit truncation, and the
/// independent retained-history cursor boundary.
#[derive(Debug, Clone, Default)]
pub struct LogTail {
    pub lines: Vec<LogLine>,
    pub truncated: bool,
    /// Last sequence committed when the snapshot was taken.
    pub high_watermark: u64,
    /// Cursor a consumer should use to ask for lines after this page.
    pub next_sequence: u64,
    /// The first sequence that retention still guarantees is readable.
    /// `None` means the journal has no retained entries yet.
    pub earliest_retained_sequence: Option<u64>,
    /// The requested numeric cursor predates retained history.  This is
    /// deliberately distinct from page-limit truncation and live lag.
    pub cursor_expired: bool,
}

#[async_trait]
pub trait LogSink: Send + Sync {
    /// Supplies the complete persisted process-name set before legacy-path
    /// lookup.  Implementations that do not persist process logs can ignore
    /// it; durable implementations use it to quarantine sanitized-name
    /// collisions rather than exposing one process's history to another.
    fn register_process_names(&self, _names: &[String]) {}

    /// Append a captured line for a managed process (keyed by process name).
    async fn append(&self, source: &str, line: LogLine) -> Result<(), LogError>;
    /// Return up to `limit` recent lines, optionally filtered to `since`, and
    /// whether older matching lines were truncated by the limit.
    async fn tail(
        &self,
        source: &str,
        limit: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
    ) -> LogTail;
    /// Live subscription to a process's new lines.
    fn subscribe(&self, source: &str) -> broadcast::Receiver<LogLine>;
    /// Creates the live receiver before returning the durable snapshot.  This
    /// gives transports one high-watermark boundary without a snapshot/live
    /// race.
    async fn subscribe_tail(
        &self,
        source: &str,
        limit: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
    ) -> (LogTail, broadcast::Receiver<LogLine>);

    /// Append a captured line for a specific job run.
    async fn append_run(&self, run_id: JobRunId, line: LogLine) -> Result<(), LogError>;
    /// Return up to `limit` recent lines for a job run.
    async fn tail_run(
        &self,
        run_id: JobRunId,
        limit: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
    ) -> LogTail;
    /// Forget a run's buffered output and remove its durable log file.
    async fn seal_run(&self, run_id: JobRunId) -> Result<(), LogError>;
    /// Remove a sealed run's durable journal.  Errors are deliberately
    /// returned to the caller so durable cleanup can be retried.
    async fn remove_run(&self, run_id: JobRunId) -> Result<(), LogError>;
    /// Live subscription to a job run's new lines.
    fn subscribe_run(&self, run_id: JobRunId) -> broadcast::Receiver<LogLine>;
    async fn subscribe_tail_run(
        &self,
        run_id: JobRunId,
        limit: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
    ) -> (LogTail, broadcast::Receiver<LogLine>);
    /// Run journals discovered on disk, including journals left by a crash.
    async fn persisted_run_ids(&self) -> Result<Vec<JobRunId>, LogError>;
}
