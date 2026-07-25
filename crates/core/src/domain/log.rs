//! Log domain values shared by process and job-run capture.

use chrono::{DateTime, Utc};
use crate::domain::JobRunId;

/// Which stream a captured line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
    /// Supervisor-injected lines (state transitions, control frames).
    System,
}

/// A single captured log line. The wire form is `{timestamp, stream, line}`;
/// the FE synthesizes its own `id`/`source` (child 04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// Monotonic sequence allocated by the source journal.  Zero is reserved
    /// for compatibility-only readers which cannot recover a durable cursor.
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub stream: LogStream,
    pub line: String,
}

impl LogLine {
    pub fn now(stream: LogStream, line: impl Into<String>) -> Self {
        LogLine {
            sequence: 0,
            timestamp: Utc::now(),
            stream,
            line: line.into(),
        }
    }
}

/// Durable retry record for a Run journal whose database owner was removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLogCleanup {
    pub run_id: JobRunId,
    pub attempts: u32,
    pub last_error: Option<String>,
}
