//! The in-memory shape a `ConfigSource` resolves a config file into. Kept in
//! `core` so the port returns domain types, not the TOML wire schema.

use serde::{Deserialize, Serialize};

use super::job::{Job, JobRunId};
use super::process::ProcessSpec;

/// A fully-parsed, validated configuration snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedConfig {
    pub processes: Vec<ProcessSpec>,
    pub jobs: Vec<Job>,
}

/// The durable definition state that a config apply can restore. Runtime names
/// are intentionally diagnostic only: a PID is never restored from a config
/// journal because its identity may no longer be valid after a restart.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    pub processes: Vec<ProcessSpec>,
    pub jobs: Vec<Job>,
    pub running_direct_processes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyMode {
    Merge,
    Replace,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigDiff {
    pub added_processes: Vec<String>,
    pub updated_processes: Vec<String>,
    pub removed_processes: Vec<String>,
    pub added_jobs: Vec<String>,
    pub updated_jobs: Vec<String>,
    pub removed_jobs: Vec<String>,
}

impl ConfigDiff {
    pub fn is_empty(&self) -> bool {
        self.added_processes.is_empty()
            && self.updated_processes.is_empty()
            && self.removed_processes.is_empty()
            && self.added_jobs.is_empty()
            && self.updated_jobs.is_empty()
            && self.removed_jobs.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigApplyStage {
    Prepared,
    SchedulerAndRegistrarStaged,
    /// A Replace removal has started cancelling owned work. From this point
    /// restoring the old snapshot would falsely claim that the old execution
    /// set still exists, so recovery must converge on `target` instead.
    ForwardRecovery,
    DirectProcessesStopped,
    DatabaseCommitted,
    NewProcessesStarted,
    CompensationFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigApplyJournal {
    pub apply_id: uuid::Uuid,
    pub previous: ConfigSnapshot,
    pub target: ConfigSnapshot,
    pub diff: ConfigDiff,
    pub stage: ConfigApplyStage,
    pub compensation_error: Option<String>,
    /// Each Direct target start is recorded before spawn and updated with the
    /// verified native generation afterwards.  This lets recovery distinguish
    /// an already-started target from an unstarted target after a daemon crash.
    #[serde(default)]
    pub target_direct_starts: Vec<ConfigTargetDirectStart>,
}

/// Durable intent and identity for one Direct process started by a config
/// apply.  `spec` is the exact target identity; the generation is filled only
/// after a successful spawn has produced a native handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigTargetDirectStart {
    pub name: String,
    pub spec: ProcessSpec,
    pub expected_generation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigApplyResult {
    pub apply_id: Option<uuid::Uuid>,
    pub mode: ApplyMode,
    pub diff: ConfigDiff,
    pub dry_run: bool,
}

/// A stable, ordered representation used by the dependency exactly-once gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencySignature {
    pub run_ids: Vec<JobRunId>,
}
