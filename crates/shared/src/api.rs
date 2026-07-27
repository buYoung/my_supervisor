//! REST wire DTOs. Snake_case JSON per `docs/API.md` §4 (authoritative). The
//! FE camelCase reconciliation is child 04's job; this crate never forks the
//! wire shape.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

fn is_false(value: &bool) -> bool {
    !*value
}

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
pub struct ProcessInstanceStatusDto {
    pub instance_id: Uuid,
    pub ordinal: u16,
    pub generation: u64,
    pub state: ProcessStateDto,
    pub pid: Option<u32>,
    pub restart_count: u32,
    pub started_at: Option<DateTime<Utc>>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GuardStateDto {
    Unknown,
    Healthy,
    Unhealthy,
    Unsupported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GuardRestartCauseDto {
    WatchChanged,
    MemoryCeiling,
    LivenessFailure,
}

/// Additive latest runtime-guard evidence.  `is_historical` prevents clients
/// from treating a persisted readiness result as current after daemon restart.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardStatusDto {
    pub process_id: Uuid,
    pub native_generation: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub liveness: GuardStateDto,
    pub readiness: GuardStateDto,
    pub memory: GuardStateDto,
    pub watch: GuardStateDto,
    pub last_restart_cause: Option<GuardRestartCauseDto>,
    pub last_error: Option<String>,
    pub is_historical: bool,
}

const fn default_desired_instances() -> u16 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessStatusDto {
    /// Older daemons omit this additive field; current daemons always supply
    /// the persisted process definition UUID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_id: Option<Uuid>,
    pub name: String,
    pub state: ProcessStateDto,
    pub management_mode: ManagementModeDto,
    /// Desired slot count. Older daemon responses deserialize as the legacy
    /// single-instance default.
    #[serde(default = "default_desired_instances")]
    pub desired_instances: u16,
    /// Exact durable slot observations, ordered by ordinal when supported by
    /// the backing repository. Older/non-supporting sources expose an empty list.
    #[serde(default)]
    pub instances: Vec<ProcessInstanceStatusDto>,
    pub pid: Option<u32>,
    pub unit_name: Option<String>,
    pub restart_count: u32,
    pub started_at: Option<DateTime<Utc>>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<GuardStatusDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessListDto {
    pub processes: Vec<ProcessStatusDto>,
}

/// Additive bounded list contract.  Legacy `ProcessListDto` remains the
/// response of `GET /processes`; operators that need refresh-safe paging use
/// the explicit page endpoint instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessPageDto {
    pub processes: Vec<ProcessStatusDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub high_watermark: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub partial: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_partitions: Vec<String>,
}

/// Additive response for `GET /processes/{name}/instances`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessInstancesDto {
    pub name: String,
    pub desired_instances: u16,
    pub instances: Vec<ProcessInstanceStatusDto>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessOperationInstanceStateDto {
    Completed,
    Failed,
    NotAttempted,
    Superseded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessOperationInstanceOutcomeDto {
    pub instance_id: Uuid,
    pub ordinal: u16,
    pub state: ProcessOperationInstanceStateDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_stage: Option<String>,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessOperationDto {
    pub operation_id: Uuid,
    pub name: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_instances: Option<u16>,
    pub phase: String,
    pub batch: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensation: Option<String>,
    pub completed: bool,
    pub outcomes: Vec<ProcessOperationInstanceOutcomeDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScaleProcessRequestDto {
    pub instances: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RollingRestartRequestDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<Uuid>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatchPolicyDto {
    pub roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recursive: Option<bool>,
    #[serde(default)]
    pub exclusions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_symlinks: Option<bool>,
    pub debounce_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryPolicyDto {
    pub ceiling_bytes: u64,
    pub sample_interval_ms: u64,
    pub consecutive_breaches: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CheckKindDto {
    Exec { command: String, args: Vec<String> },
    Tcp { host: String, port: u16 },
    Http { url: String, expected_status: u16 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckPolicyDto {
    pub kind: CheckKindDto,
    pub interval_ms: u64,
    pub timeout_ms: u64,
    pub consecutive_successes: u16,
    pub consecutive_failures: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RollingPolicyDto {
    pub max_surge: u16,
    pub max_unavailable: u16,
    pub readiness_timeout_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routability: Option<bool>,
}

/// POST `/api/v1/processes` body — one `[[process]]` config entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessConfigDto {
    /// Optional so legacy config remains name-keyed; omitted IDs are derived
    /// deterministically by the conversion boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_id: Option<Uuid>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instances: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch: Option<WatchPolicyDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPolicyDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liveness: Option<CheckPolicyDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readiness: Option<CheckPolicyDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rolling: Option<RollingPolicyDto>,
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
    /// First numeric cursor still retained by the journal. It is absent from
    /// older servers and intentionally additive for compatibility.
    #[serde(default)]
    pub earliest_retained_sequence: Option<u64>,
    /// True when `after_sequence` predates retained journal history.
    #[serde(default)]
    pub cursor_expired: bool,
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
pub enum MisfirePolicyDto {
    Skip,
    RunOnce,
    CatchUp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueueOverflowDto {
    RejectNew,
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetryPolicyDto {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u16,
    #[serde(default = "default_retry_initial_backoff_sec")]
    pub initial_backoff_sec: u64,
    #[serde(default = "default_retry_max_backoff_sec")]
    pub max_backoff_sec: u64,
    #[serde(default = "default_retry_multiplier")]
    pub multiplier: u8,
    #[serde(default = "default_retry_jitter_percent")]
    pub jitter_percent: u8,
}
fn default_max_attempts() -> u16 {
    1
}
fn default_retry_initial_backoff_sec() -> u64 {
    1
}
fn default_retry_max_backoff_sec() -> u64 {
    300
}
fn default_retry_multiplier() -> u8 {
    2
}
fn default_retry_jitter_percent() -> u8 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdmissionPolicyDto {
    pub max_concurrency: u16,
    pub max_queue: u16,
    pub overflow: QueueOverflowDto,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default)]
    pub schedule_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub misfire_policy: Option<MisfirePolicyDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<RetryPolicyDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission: Option<AdmissionPolicyDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobListDto {
    pub jobs: Vec<JobStatusDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobPageDto {
    pub jobs: Vec<JobStatusDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub high_watermark: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub partial: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_partitions: Vec<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_scheduled_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence_trigger_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence_schedule_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence_attempt: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobRunListDto {
    pub runs: Vec<JobRunDto>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobRunPageDto {
    pub runs: Vec<JobRunDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub high_watermark: String,
}

/// Bootstrap metadata for a browser-debug session.  The opaque session id is
/// carried only in the HttpOnly cookie; this DTO deliberately exposes only
/// the non-secret CSRF nonce held in renderer memory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionBootstrapDto {
    pub csrf_token: String,
    pub expires_at: DateTime<Utc>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub misfire_policy: Option<MisfirePolicyDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<RetryPolicyDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission: Option<AdmissionPolicyDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobPreviewRequestDto {
    pub config: JobConfigDto,
    pub at: DateTime<Utc>,
    #[serde(default = "default_preview_count")]
    pub count: u16,
}
fn default_preview_count() -> u16 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobPreviewOccurrenceDto {
    pub scheduled_at: DateTime<Utc>,
    pub local_time: String,
    pub timezone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobPreviewDto {
    pub occurrences: Vec<JobPreviewOccurrenceDto>,
}

// ---------------------------------------------------------------------------
// Daemon
// ---------------------------------------------------------------------------

/// Read-only local-owner discovery data. It intentionally excludes the bearer
/// secret; native clients read that from the user-only control file instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnerDiscoveryDto {
    pub endpoint: String,
    pub version: String,
    pub pid: u32,
    pub native_start_identity: String,
    pub credential_generation: u64,
}

/// Stable local ownership outcome for native host diagnostics. The HTTP API
/// does not expose this unauthenticated; it is stored as owner metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OwnershipResultDto {
    Acquired,
    Contended { owner: Option<OwnerDiscoveryDto> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonStatusDto {
    pub version: String,
    pub started_at: DateTime<Utc>,
    pub pid: u32,
    pub process_count: u32,
    pub config_path: String,
    pub log_dir: String,
}

/// User-scoped supervisor service state.  It is intentionally independent of
/// managed-process states because launchd registration can exist before the
/// daemon has accepted an authenticated health request.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStateDto {
    NotInstalled,
    Stopped,
    Starting,
    Ready,
    Degraded,
    Stopping,
    Failed,
    Incompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceStatusDto {
    pub state: ServiceStateDto,
    pub label: String,
    pub plist_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<OwnerDiscoveryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenRotationDto {
    pub credential_generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupResultDto {
    pub backup_id: String,
    pub manifest_path: String,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpgradeJournalDto {
    pub phase: String,
    pub active_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_path: Option<String>,
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

// ---------------------------------------------------------------------------
// Observability (additive; existing live event envelopes remain unchanged)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverityDto {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AlertStateDto {
    Active,
    AcknowledgedActive,
    Resolved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlertRuleDto {
    pub id: Uuid,
    pub name: String,
    pub condition: String,
    pub severity: AlertSeverityDto,
    pub cooldown_seconds: u64,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpsertAlertRuleRequestDto {
    pub id: Option<Uuid>,
    pub name: String,
    pub condition: String,
    pub severity: AlertSeverityDto,
    pub cooldown_seconds: u64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}
fn default_enabled() -> bool {
    true
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperatorEventDto {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub source: String,
    pub kind: String,
    pub severity: AlertSeverityDto,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricSampleDto {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub source: String,
    pub cpu_percent: Option<f64>,
    pub memory_bytes: Option<u64>,
    pub partial_bucket: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlertEpisodeDto {
    pub id: Uuid,
    pub rule_id: Uuid,
    pub source: String,
    pub cause: String,
    pub state: AlertStateDto,
    pub severity: AlertSeverityDto,
    pub opened_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub acknowledged_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeliveryAttemptDto {
    pub id: Uuid,
    pub alert_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub kind: String,
    pub outcome: String,
    pub detail: Option<String>,
    pub lease_until: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObservabilityPageDto<T> {
    pub records: Vec<T>,
    pub next_cursor: Option<String>,
    pub high_watermark: Option<String>,
    pub earliest_retained_at: Option<DateTime<Utc>>,
}
