//! Process domain entities (Direct + SystemRegistered management modes).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// How a managed process is controlled.
///
/// `Direct` — the daemon spawns / supervises / restarts the child itself.
/// `SystemRegistered` — the OS service manager owns the lifecycle; control and
/// status flow through `ProcessServiceRegistrar` keyed on `unit_name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagementMode {
    Direct,
    SystemRegistered { unit_name: String },
}

impl Default for ManagementMode {
    fn default() -> Self {
        ManagementMode::Direct
    }
}

/// Tied children die with the daemon; detached children outlive it.
/// Only meaningful in `Direct` mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LifecycleMode {
    #[default]
    Tied,
    Detached,
}

/// Observable runtime state of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Starting,
    Running,
    Stopping,
    Crashed,
    Stopped,
}

/// Signal used to ask a child to stop before escalating to a hard kill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShutdownSignal {
    #[default]
    Term,
    Int,
    Kill,
}

/// Restart behavior. In `SystemRegistered` mode this is translated to an OS
/// `Restart=` directive and the in-daemon restart engine is a no-op (DD-025).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartPolicy {
    pub enabled: bool,
    pub max_retries: Option<u32>,
    pub backoff_initial: Duration,
    pub backoff_max: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        RestartPolicy {
            enabled: true,
            max_retries: None,
            backoff_initial: Duration::from_secs(1),
            backoff_max: Duration::from_secs(60),
        }
    }
}

/// Graceful-shutdown policy applied before a force kill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownPolicy {
    pub signal: ShutdownSignal,
    pub grace_period: Duration,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
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
}

impl ProcessSpec {
    /// Minimal Direct-mode spec for the common case.
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        ProcessSpec {
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
        }
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
    pub started_at: DateTime<Utc>,
}

/// A point-in-time status snapshot for the API/UI.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessStatus {
    pub name: String,
    pub state: ProcessState,
    pub management_mode: ManagementMode,
    pub pid: Option<u32>,
    pub unit_name: Option<String>,
    pub restart_count: u32,
    pub started_at: Option<DateTime<Utc>>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}
