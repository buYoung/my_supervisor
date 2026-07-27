//! Process domain entities (Direct + SystemRegistered management modes).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How a managed process is controlled.
///
/// `Direct` — the daemon spawns / supervises / restarts the child itself.
/// `SystemRegistered` — the OS service manager owns the lifecycle; control and
/// status flow through `ProcessServiceRegistrar` keyed on `unit_name`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ManagementMode {
    #[default]
    Direct,
    SystemRegistered {
        unit_name: String,
    },
}

/// Tied children die with the daemon; detached children outlive it.
/// Only meaningful in `Direct` mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LifecycleMode {
    #[default]
    Tied,
    Detached,
}

/// Stable identity of a persisted process definition.  It is separate from a
/// user-editable name so storage and instance allocation retain a durable key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProcessDefinitionId(pub Uuid);

impl ProcessDefinitionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Stable compatibility identity for name-keyed definitions created before
    /// durable IDs were introduced.  New programmatic definitions use `new`.
    pub fn from_legacy_name(name: &str) -> Self {
        const OFFSET_A: u64 = 0xcbf2_9ce4_8422_2325;
        const OFFSET_B: u64 = 0x8422_2325_cbf2_9ce4;
        const PRIME_A: u64 = 0x0000_0100_0000_01b3;
        const PRIME_B: u64 = 0x0000_0100_0000_01c3;

        let (mut first, mut second) = (OFFSET_A, OFFSET_B);
        for byte in name.bytes() {
            first = (first ^ u64::from(byte)).wrapping_mul(PRIME_A);
            second = (second ^ u64::from(byte)).wrapping_mul(PRIME_B);
        }
        Self(Uuid::from_u128(
            (u128::from(first) << 64) | u128::from(second),
        ))
    }
}

impl Default for ProcessDefinitionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Stable identity of one allocated process slot.  The ID is never reused
/// when an ordinal is retired and later allocated again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProcessInstanceId(pub Uuid);

impl ProcessInstanceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ProcessInstanceId {
    fn default() -> Self {
        Self::new()
    }
}

/// A durable Direct-mode slot.  Ordinal zero is the compatibility slot for
/// legacy single-instance definitions; generation starts at one and is owned
/// by the repository as later runtime work advances the slot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProcessInstance {
    pub id: ProcessInstanceId,
    pub definition_id: ProcessDefinitionId,
    pub ordinal: u16,
    pub generation: u64,
}

/// Observable runtime state of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessState {
    Starting,
    Running,
    Stopping,
    Crashed,
    Stopped,
}

/// Signal used to ask a child to stop before escalating to a hard kill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShutdownSignal {
    #[default]
    Term,
    Int,
    Kill,
}

/// Restart behavior. Direct-mode delays are produced by the application's
/// backoff library. System-registered mode delegates restarting to the OS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartPolicy {
    pub enabled: bool,
    pub max_retries: Option<u32>,
    pub backoff_initial: Duration,
    pub backoff_max: Duration,
    pub backoff_multiplier: u32,
    pub jitter: bool,
    pub reset_after: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        RestartPolicy {
            enabled: true,
            max_retries: None,
            backoff_initial: Duration::from_secs(1),
            backoff_max: Duration::from_secs(60),
            backoff_multiplier: 2,
            jitter: true,
            reset_after: Duration::from_secs(60),
        }
    }
}

/// Graceful-shutdown policy applied before a force kill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShutdownPolicy {
    pub signal: ShutdownSignal,
    pub grace_period: Duration,
}

/// Restart a process when changes below explicit roots are observed.  Paths
/// are intentionally absolute so a persisted definition has no dependency on
/// the daemon's working directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchPolicy {
    pub roots: Vec<PathBuf>,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub exclusions: Vec<PathBuf>,
    #[serde(default)]
    pub follow_symlinks: bool,
    pub debounce: Duration,
}

/// Aggregate resident-memory limit for the process group, including children.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryPolicy {
    pub ceiling_bytes: u64,
    pub sample_interval: Duration,
    pub consecutive_breaches: u16,
}

/// An active liveness or readiness probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckKind {
    Exec { command: String, args: Vec<String> },
    Tcp { host: String, port: u16 },
    Http { url: String, expected_status: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckPolicy {
    pub kind: CheckKind,
    pub interval: Duration,
    pub timeout: Duration,
    pub consecutive_successes: u16,
    pub consecutive_failures: u16,
}

/// Current bounded evaluator result. `Unknown` is deliberately distinct from
/// a passing result: after a daemon restart persisted evidence is historical
/// until the newly-owned generation observes a fresh result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardState {
    Unknown,
    Healthy,
    Unhealthy,
    Unsupported,
}

/// Stable cause recorded for an automatic guardrail restart.  Manual and
/// crash-loop restarts retain their existing semantics and are not invented as
/// guard causes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardRestartCause {
    WatchChanged,
    MemoryCeiling,
    LivenessFailure,
}

/// Latest bounded runtime-guard evidence for one native child generation.
/// This is a value object so storage adapters can retain it without depending
/// on application events or task ownership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardSnapshot {
    /// Stable slot identity is additive so snapshots written before
    /// multi-instance ownership remain readable as ordinal-zero history.
    #[serde(default)]
    pub instance_id: Option<ProcessInstanceId>,
    #[serde(default)]
    pub logical_generation: Option<u64>,
    pub process_id: Uuid,
    pub native_generation: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub liveness: GuardState,
    pub readiness: GuardState,
    pub memory: GuardState,
    pub watch: GuardState,
    pub last_restart_cause: Option<GuardRestartCause>,
    pub last_error: Option<String>,
}

/// Bounds used by a readiness-gated rolling replacement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollingPolicy {
    pub max_surge: u16,
    pub max_unavailable: u16,
    pub readiness_timeout: Duration,
    #[serde(default)]
    pub routability: bool,
}

impl Default for ShutdownPolicy {
    fn default() -> Self {
        ShutdownPolicy {
            signal: ShutdownSignal::Term,
            grace_period: Duration::from_secs(10),
        }
    }
}

/// A managed-process definition. Mirrors a single `[[process]]` config block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSpec {
    #[serde(default)]
    pub definition_id: ProcessDefinitionId,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub management_mode: ManagementMode,
    pub lifecycle: LifecycleMode,
    pub autostart: bool,
    pub restart: RestartPolicy,
    pub shutdown: ShutdownPolicy,
    /// Desired number of independently identified Direct-mode process slots.
    #[serde(default = "default_instances")]
    pub instances: u16,
    #[serde(default)]
    pub watch: Option<WatchPolicy>,
    #[serde(default)]
    pub memory: Option<MemoryPolicy>,
    #[serde(default)]
    pub liveness: Option<CheckPolicy>,
    #[serde(default)]
    pub readiness: Option<CheckPolicy>,
    #[serde(default)]
    pub rolling: Option<RollingPolicy>,
}

const fn default_instances() -> u16 {
    1
}

impl ProcessSpec {
    /// Minimal Direct-mode spec for the common case.
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        ProcessSpec {
            definition_id: ProcessDefinitionId::new(),
            name: name.into(),
            command: command.into(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            management_mode: ManagementMode::Direct,
            lifecycle: LifecycleMode::Tied,
            autostart: false,
            restart: RestartPolicy::default(),
            shutdown: ShutdownPolicy::default(),
            instances: default_instances(),
            watch: None,
            memory: None,
            liveness: None,
            readiness: None,
            rolling: None,
        }
    }

    /// Validates production-only settings before a definition is persisted or
    /// a management-mode conversion performs an external mutation.
    pub fn validate_production_policy(&self) -> Result<(), &'static str> {
        if !(1..=128).contains(&self.instances) {
            return Err("instances must be between 1 and 128");
        }
        let direct_only = self.instances != 1
            || self.watch.is_some()
            || self.memory.is_some()
            || self.liveness.is_some()
            || self.readiness.is_some()
            || self.rolling.is_some();
        if matches!(
            self.management_mode,
            ManagementMode::SystemRegistered { .. }
        ) && direct_only
        {
            return Err("production supervision policies require Direct management mode");
        }
        if let Some(watch) = &self.watch {
            if watch.roots.is_empty()
                || watch.roots.len() > 256
                || watch.debounce.is_zero()
                || watch.follow_symlinks
                || watch.roots.iter().any(|root| !root.is_absolute())
            {
                return Err("watch policy has invalid roots or debounce");
            }
        }
        if let Some(memory) = &self.memory {
            if memory.ceiling_bytes == 0
                || memory.sample_interval.is_zero()
                || memory.consecutive_breaches == 0
            {
                return Err("memory policy values must be greater than zero");
            }
        }
        for check in [&self.liveness, &self.readiness].into_iter().flatten() {
            if check.interval.is_zero()
                || check.timeout.is_zero()
                || check.timeout >= check.interval
                || check.consecutive_successes == 0
                || check.consecutive_failures == 0
            {
                return Err("check policy interval, timeout, and thresholds must be valid");
            }
        }
        if let Some(rolling) = &self.rolling {
            if self.readiness.is_none()
                || rolling.max_unavailable >= self.instances
                || rolling.max_surge == 0
                || rolling.readiness_timeout.is_zero()
            {
                return Err("rolling policy requires readiness and valid capacity");
            }
        }
        Ok(())
    }
}

/// Lightweight identity of a running Direct-mode child.
///
/// The opaque OS handle (tokio child, job object, …) is owned inside the
/// `platform/*` adapter; this struct is the cross-layer reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildHandle {
    pub process_id: Uuid,
    pub pid: u32,
    /// Dedicated process group created by the platform adapter.  Missing values
    /// represent handles written by an older daemon and are never signalable.
    pub pgid: Option<u32>,
    /// Platform-provided process creation token.  It protects against PID reuse
    /// after a daemon restart; older persisted handles deliberately omit it.
    pub generation: Option<String>,
    pub started_at: DateTime<Utc>,
}

/// A deferred durable-handle removal after the operating system has already
/// confirmed the child exited.  The process identity is retained so a retry
/// can never clear a newer handle written for the same process name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHandleCleanup {
    pub name: String,
    pub process_id: Uuid,
    pub generation: Option<String>,
    pub attempts: u32,
    pub last_error: Option<String>,
}

/// A point-in-time status snapshot for the API/UI.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessInstanceStatus {
    pub instance_id: ProcessInstanceId,
    pub ordinal: u16,
    pub generation: u64,
    pub state: ProcessState,
    pub pid: Option<u32>,
    pub restart_count: u32,
    pub started_at: Option<DateTime<Utc>>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

/// The public group mutation that owns a durable, replayable result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessOperationKind {
    Scale,
    RollingRestart,
}

/// Deterministic status for one slot in a group operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessOperationInstanceState {
    Completed,
    Failed,
    NotAttempted,
    Superseded,
}

/// Per-instance outcome.  The stable slot identity lets consumers distinguish
/// a failed replacement from an untouched sibling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOperationInstanceOutcome {
    pub instance_id: ProcessInstanceId,
    pub ordinal: u16,
    pub state: ProcessOperationInstanceState,
    pub failed_stage: Option<String>,
    pub retryable: bool,
}

/// Crash-safe operation/rollout record.  `phase` and `compensation` are kept
/// explicit so a later daemon can fail closed instead of treating an
/// interrupted replacement as success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOperation {
    pub operation_id: Uuid,
    pub name: String,
    pub kind: ProcessOperationKind,
    pub target_instances: Option<u16>,
    pub phase: String,
    pub batch: u32,
    pub deadline: Option<DateTime<Utc>>,
    pub old_instance_ids: Vec<ProcessInstanceId>,
    pub new_instance_ids: Vec<ProcessInstanceId>,
    pub compensation: Option<String>,
    pub outcomes: Vec<ProcessOperationInstanceOutcome>,
    pub completed: bool,
}

/// A point-in-time aggregate and per-instance status snapshot for the API/UI.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessStatus {
    pub definition_id: ProcessDefinitionId,
    pub name: String,
    pub state: ProcessState,
    pub management_mode: ManagementMode,
    pub desired_instances: u16,
    pub instances: Vec<ProcessInstanceStatus>,
    pub pid: Option<u32>,
    pub unit_name: Option<String>,
    pub restart_count: u32,
    pub started_at: Option<DateTime<Utc>>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    /// Latest guard evidence. It remains available after restart but callers
    /// must honor `is_historical` until this daemon observes the active
    /// generation again.
    pub guard: Option<GuardStatus>,
}

/// Public guard status, combining durable evidence with the current daemon's
/// ownership knowledge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardStatus {
    pub snapshot: GuardSnapshot,
    pub is_historical: bool,
}

/// CPU and resident-memory usage sampled from the operating system.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ProcessResourceUsage {
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}
