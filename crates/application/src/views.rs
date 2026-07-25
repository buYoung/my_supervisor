//! Facade return types. Domain compositions only — no wire/serde here; the
//! http adapter (and the Tauri invoke adapter) map these onto `shared` DTOs.

use chrono::{DateTime, Utc};

use my_supervisor_core::domain::{Job, JobRun, LogLine};

/// A job plus the scheduler/history-derived fields the API surfaces.
#[derive(Debug, Clone)]
pub struct JobView {
    pub job: Job,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_run: Option<JobRun>,
    pub success_rate_recent: Option<f32>,
    pub upstream: Vec<String>,
    pub downstream: Vec<String>,
}

/// A page of log lines with truncation/backpressure metadata.
#[derive(Debug, Clone)]
pub struct LogPage {
    pub lines: Vec<LogLine>,
    pub truncated: bool,
    pub dropped_count: u64,
    pub high_watermark: u64,
    pub next_sequence: u64,
}

/// Daemon identity + live counts.
#[derive(Debug, Clone)]
pub struct DaemonInfo {
    pub version: String,
    pub started_at: DateTime<Utc>,
    pub pid: u32,
    pub process_count: u32,
    pub config_path: String,
    pub log_dir: String,
}

/// One durable recovery record safe to expose to an operator. Native handles,
/// target commands, and environment values deliberately stay internal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryDiagnostic {
    pub kind: String,
    pub id: String,
    pub resource: String,
    pub stage: String,
    pub attempts: u32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryDiagnostics {
    pub records: Vec<RecoveryDiagnostic>,
}

/// Outcome of a restart request — Direct restarts are accepted; SystemRegistered
/// restarts are a documented no-op (DD-025).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartOutcome {
    Accepted,
    Noop { reason: String },
}

/// Target management mode for a `convert` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvertTarget {
    Direct,
    SystemRegistered,
}
