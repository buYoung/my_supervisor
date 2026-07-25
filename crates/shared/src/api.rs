//! REST wire DTOs. Snake_case JSON per `docs/API.md` §4 (authoritative). The
//! FE camelCase reconciliation is child 04's job; this crate never forks the
//! wire shape.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Processes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessStateDto {
    Starting,
    Running,
    Stopping,
    Crashed,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ManagementModeDto {
    Direct,
    SystemRegistered { unit_name: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleModeDto {
    Tied,
    Detached,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessStatusDto {
    pub name: String,
    pub state: ProcessStateDto,
    pub management_mode: ManagementModeDto,
    pub pid: Option<u32>,
    pub unit_name: Option<String>,
    pub restart_count: u32,
    pub started_at: Option<DateTime<Utc>>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessListDto {
    pub processes: Vec<ProcessStatusDto>,
}

/// Restart settings shared by TOML config and process-registration requests.
/// Jitter follows the selected backoff library's timing behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RestartPolicyDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff_initial_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff_max_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff_multiplier: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_after_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownSignalDto {
    Term,
    Int,
    Kill,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ShutdownPolicyDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<ShutdownSignalDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grace_period_ms: Option<u64>,
}

/// POST `/api/v1/processes` body — one `[[process]]` config entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessConfigDto {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub management_mode: Option<ManagementModeDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<LifecycleModeDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autostart: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<RestartPolicyDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shutdown: Option<ShutdownPolicyDto>,
}

/// Response of `POST /api/v1/processes/{name}/restart` for SystemRegistered
/// processes (DD-025). Direct restarts return `202 Accepted` with no body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestartNoopDto {
    pub noop: bool,
    pub reason: String,
}

/// Target mode for `POST /api/v1/processes/{name}/convert`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConvertTargetDto {
    Direct,
    SystemRegistered,
}

/// Body of `POST /api/v1/processes/{name}/convert`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConvertRequestDto {
    pub to: ConvertTargetDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_start: Option<bool>,
}

// ---------------------------------------------------------------------------
// Logs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogStreamDto {
    Stdout,
    Stderr,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogLineDto {
    /// Source-local durable cursor.  Older servers may omit it, so clients
    /// must treat zero as a compatibility-only value rather than a cursor.
    #[serde(default)]
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub stream: LogStreamDto,
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogsResponseDto {
    pub lines: Vec<LogLineDto>,
    pub truncated: bool,
    pub dropped_count: u64,
    /// Last durable sequence included in the snapshot boundary.
    #[serde(default)]
    pub high_watermark: u64,
    /// Cursor for the next REST gap-recovery request.
    #[serde(default)]
    pub next_sequence: u64,
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobTriggerDto {
    Cron { expr: String },
    Interval { every_sec: u64 },
    OneShot { at: DateTime<Utc> },
    DependsOn { jobs: Vec<String> },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OnOverlapDto {
    Skip,
    Queue,
    Parallel,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OnDependencyFailureDto {
    Skip,
    RunAnyway,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobRunStateDto {
    Pending,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggeredByDto {
    Schedule,
    Manual,
    Dependency { upstream_run_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobRunSummaryDto {
    pub run_id: String,
    pub state: JobRunStateDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobDependenciesDto {
    pub upstream: Vec<String>,
    pub downstream: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobStatusDto {
    pub name: String,
    pub trigger: JobTriggerDto,
    pub on_overlap: OnOverlapDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run: Option<JobRunSummaryDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success_rate_recent: Option<f32>,
    pub dependencies: JobDependenciesDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobListDto {
    pub jobs: Vec<JobStatusDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobRunDto {
    pub run_id: String,
    pub job_name: String,
    pub triggered_by: TriggeredByDto,
    pub scheduled_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub state: JobRunStateDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobRunListDto {
    pub runs: Vec<JobRunDto>,
    pub truncated: bool,
}

// ---------------------------------------------------------------------------
// Config apply
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConfigApplyModeDto {
    #[default]
    Merge,
    Replace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ConfigDiffDto {
    #[serde(default)]
    pub added_processes: Vec<String>,
    #[serde(default)]
    pub updated_processes: Vec<String>,
    #[serde(default)]
    pub removed_processes: Vec<String>,
    #[serde(default)]
    pub added_jobs: Vec<String>,
    #[serde(default)]
    pub updated_jobs: Vec<String>,
    #[serde(default)]
    pub removed_jobs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigApplyResultDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply_id: Option<String>,
    pub mode: ConfigApplyModeDto,
    pub diff: ConfigDiffDto,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LogRetentionDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_runs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age_days: Option<u32>,
}

/// POST `/api/v1/jobs` body — one `[[job]]` config entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobConfigDto {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub trigger: JobTriggerDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_overlap: Option<OnOverlapDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_dependency_failure: Option<OnDependencyFailureDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_sec: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_retention: Option<LogRetentionDto>,
}

// ---------------------------------------------------------------------------
// Daemon
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonStatusDto {
    pub version: String,
    pub started_at: DateTime<Utc>,
    pub pid: u32,
    pub process_count: u32,
    pub config_path: String,
    pub log_dir: String,
}

/// A bounded, pending recovery record. It deliberately excludes command,
/// environment, PID, and native identity details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryDiagnosticDto {
    pub kind: String,
    pub id: String,
    pub resource: String,
    pub stage: String,
    pub attempts: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RecoveryDiagnosticsDto {
    pub records: Vec<RecoveryDiagnosticDto>,
}
