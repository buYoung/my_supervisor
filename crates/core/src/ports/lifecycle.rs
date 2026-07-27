//! `LifecycleController` — Direct-mode spawn / probe / reap. Each OS implements
//! the tied/detached strategy with its own kernel primitives (DD-018).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{broadcast, watch};

use crate::domain::{
    CheckPolicy, ChildHandle, JobId, JobRunId, JobRunState, LogLine, ProcessResourceUsage,
    ProcessSpec, WatchPolicy,
};
use crate::ports::log_sink::LogTail;

/// Result of an aliveness probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aliveness {
    Alive,
    Dead,
}

/// Outcome of a transient (run-to-completion) execution used by `JobRunner`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransientOutcome {
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub exit_code: Option<i32>,
}

/// Durable progress marker for transient-process cleanup.  The stages are
/// intentionally platform-neutral: adapters own live child/pump tasks while
/// the application owns the retryable journal and terminal Run transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransientCleanupStage {
    TerminateGroup,
    WaitLeader,
    JoinPumps,
    SealLog,
    PersistTerminal,
}

/// A restart-safe description of cleanup still required for one Job Run.
/// `ChildHandle` is a verified native identity, never just a recycled PID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupTicket {
    pub cleanup_id: uuid::Uuid,
    pub job_id: JobId,
    pub job_name: String,
    pub run_id: JobRunId,
    pub child: ChildHandle,
    pub stage: TransientCleanupStage,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub intended_terminal_state: JobRunState,
    /// The outcome observed before cleanup became durable.  Recovery must not
    /// replace this with its own retry time or discard a non-zero exit code.
    pub outcome: TransientOutcome,
}

/// Controlled completion result for a transient child.  `Unreaped` is
/// deliberately distinct from a failed program exit: the adapter still owns
/// the child and callers must retain its active-run entry for a later retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransientCompletion {
    Exited(TransientOutcome),
    TimedOut(TransientOutcome),
    Cancelled(TransientOutcome),
    CleanupPending {
        cause: String,
        stage: TransientCleanupStage,
        intended_terminal_state: JobRunState,
        outcome: TransientOutcome,
    },
}

#[derive(Debug, Error)]
pub enum SpawnError {
    #[error("working directory does not exist: {0}")]
    CwdMissing(String),
    #[error("failed to spawn '{name}': {message}")]
    Io { name: String, message: String },
}

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("probe failed: {0}")]
    Failed(String),
}

#[derive(Debug, Error)]
pub enum ReapError {
    #[error("reap failed: {0}")]
    Failed(String),
}

/// A resource sample for the complete dedicated process group.  `Unknown` is
/// intentionally distinct from a zero value: guard policy must reset its
/// sustained-breach window when native membership or RSS cannot be verified.
#[derive(Debug, Clone, PartialEq)]
pub enum OwnedGroupResourceUsage {
    Sample(ProcessResourceUsage),
    Unknown { reason: String },
}

/// Bounded diagnostics retained from a one-shot external check.  The adapter
/// owns truncation so a check can never make supervisor memory unbounded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckDiagnostics {
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

/// Protocol-specific completion metadata from a successful or failed check.
/// Exit codes and HTTP statuses intentionally do not share a lossy numeric
/// representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    ExitCode(i32),
    HttpStatus(u16),
}

/// A one-shot external-check result. Timeout and caller cancellation remain
/// typed outcomes so policy ownership can distinguish them from an unhealthy
/// result and from cleanup that still needs attention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    Succeeded {
        status: Option<CheckStatus>,
        diagnostics: CheckDiagnostics,
    },
    Failed {
        status: Option<CheckStatus>,
        diagnostics: CheckDiagnostics,
    },
    TimedOut {
        diagnostics: CheckDiagnostics,
    },
    Cancelled {
        diagnostics: CheckDiagnostics,
    },
    CleanupFailed {
        cause: String,
        diagnostics: CheckDiagnostics,
    },
}

/// Opaque native-watch ownership token.  U08 owns its lifecycle per process
/// generation; adapters only create, read, and stop the underlying watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WatchRegistrationId(pub uuid::Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchRescanReason {
    BackendOverflow,
    BackendRestart,
    ChannelOverflow,
}

/// A bounded batch of normalized filesystem observations.  A rescan reason
/// means event loss may have occurred; the adapter has completed one bounded
/// snapshot/diff before returning the observation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WatchObservation {
    pub changed_paths: Vec<PathBuf>,
    pub rescan_reason: Option<WatchRescanReason>,
}

#[derive(Debug, Error)]
pub enum GuardError {
    #[error("runtime guard capability is unsupported: {0}")]
    Unsupported(&'static str),
    #[error("runtime guard failed: {0}")]
    Failed(String),
}

/// Direct-mode process control. SystemRegistered processes use
/// `ProcessServiceRegistrar` instead.
#[async_trait]
pub trait LifecycleController: Send + Sync {
    /// Spawn a child tied to the daemon (dies when the daemon exits).
    async fn spawn_tied(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError>;
    /// Spawn a child that survives daemon exit.
    async fn spawn_detached(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError>;
    /// Check whether a previously-spawned child is still alive.
    async fn probe_alive(&self, handle: &ChildHandle) -> Result<Aliveness, ProbeError>;
    /// Read logs written directly by a detached child so they remain available
    /// while the daemon is not running.
    /// Read a detached process's durable journal. `known_process_names` is
    /// required only to make the old sanitized raw-file fallback safe: a
    /// colliding legacy filename must not be attributed to either process.
    async fn tail_detached_logs(
        &self,
        spec: &ProcessSpec,
        lines: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
        known_process_names: &[String],
    ) -> Result<LogTail, ProbeError>;
    /// Follow lines appended to a detached process's single journal. The
    /// returned receiver is established before the caller takes its snapshot.
    async fn subscribe_detached_logs(
        &self,
        spec: &ProcessSpec,
    ) -> Result<broadcast::Receiver<LogLine>, ProbeError>;
    /// Sample CPU and resident memory only after validating the child identity.
    async fn resource_usage(
        &self,
        handle: &ChildHandle,
    ) -> Result<ProcessResourceUsage, ProbeError>;
    /// Sample aggregate RSS for the verified dedicated process group.  This is
    /// additive because legacy lifecycle implementers only provide leader
    /// status sampling.
    async fn owned_group_resource_usage(
        &self,
        _handle: &ChildHandle,
    ) -> Result<OwnedGroupResourceUsage, GuardError> {
        Err(GuardError::Unsupported(
            "owned process-group resource sampling",
        ))
    }
    /// Run one bounded liveness/readiness check.  The caller retains the
    /// cancellation receiver; adapters observe it without replacing it.
    async fn run_check(
        &self,
        _policy: &CheckPolicy,
        _cancellation: &mut watch::Receiver<bool>,
    ) -> Result<CheckOutcome, GuardError> {
        Err(GuardError::Unsupported("bounded external checks"))
    }
    /// Register an explicit-root watch.  The returned opaque token is owned by
    /// the caller and must later be passed to `stop_watch`.
    async fn register_watch(
        &self,
        _policy: &WatchPolicy,
    ) -> Result<WatchRegistrationId, GuardError> {
        Err(GuardError::Unsupported("filesystem watch"))
    }
    /// Read the next debounced bounded watch batch without deciding any
    /// restart action.  Empty batches are valid while a debounce window is
    /// still open or no native event has arrived.
    async fn read_watch(
        &self,
        _registration: WatchRegistrationId,
    ) -> Result<WatchObservation, GuardError> {
        Err(GuardError::Unsupported("filesystem watch"))
    }
    /// Release native watcher resources and prevent later observations.
    async fn stop_watch(&self, _registration: WatchRegistrationId) -> Result<(), GuardError> {
        Err(GuardError::Unsupported("filesystem watch"))
    }
    /// Clean up the given handles during daemon shutdown.
    async fn reap_on_shutdown(&self, handles: &[ChildHandle]) -> Result<(), ReapError>;
    /// Spawn a transient process and retain its child/pump ownership in the
    /// adapter until `complete_transient` has reaped it.  The returned handle
    /// is published to the active-run registry before completion starts.
    async fn start_transient(
        &self,
        spec: &ProcessSpec,
        run_id: JobRunId,
    ) -> Result<ChildHandle, SpawnError>;
    /// Wait for a previously-started transient child.  Cancellation and timeout
    /// use the same adapter-owned TERM → grace → KILL → wait → pump-join path.
    /// A cleanup failure keeps adapter ownership and returns `Unreaped` rather
    /// than falsely reporting a terminal failed run.
    async fn complete_transient(
        &self,
        handle: &ChildHandle,
        timeout: Option<Duration>,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<TransientCompletion, SpawnError>;
    /// Resume cleanup using adapter-owned tasks when they survived, or native
    /// process identity when a daemon restart discarded those tasks.
    async fn resume_transient_cleanup(
        &self,
        ticket: &CleanupTicket,
    ) -> Result<TransientCompletion, SpawnError>;
}
