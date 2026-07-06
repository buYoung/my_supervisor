//! `AppDeps` — the injected port adapters the facade is built from. Cfg-free:
//! the `#[cfg(target_os)]` adapter selection lives in each host's `build_deps`.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use my_supervisor_core::ports::{
    ConfigSource, JobRepository, LifecycleController, LogSink, ProcessServiceRegistrar, Scheduler,
    ShutdownSignaler, StateRepository, SystemClock,
};

/// Static daemon identity reported by `GET /api/v1/daemon/status`.
#[derive(Debug, Clone)]
pub struct DaemonMeta {
    pub version: String,
    pub started_at: DateTime<Utc>,
    pub pid: u32,
    pub config_path: PathBuf,
    pub log_dir: PathBuf,
}

impl DaemonMeta {
    pub fn new(config_path: PathBuf, log_dir: PathBuf) -> Self {
        DaemonMeta {
            version: env!("CARGO_PKG_VERSION").to_string(),
            started_at: Utc::now(),
            pid: std::process::id(),
            config_path,
            log_dir,
        }
    }
}

/// All adapters the facade needs, as trait objects.
#[derive(Clone)]
pub struct AppDeps {
    pub lifecycle: Arc<dyn LifecycleController>,
    pub shutdown: Arc<dyn ShutdownSignaler>,
    pub registrar: Arc<dyn ProcessServiceRegistrar>,
    pub state_repo: Arc<dyn StateRepository>,
    pub job_repo: Arc<dyn JobRepository>,
    pub scheduler: Arc<dyn Scheduler>,
    pub log_sink: Arc<dyn LogSink>,
    pub clock: Arc<dyn SystemClock>,
    pub config: Arc<dyn ConfigSource>,
    pub meta: DaemonMeta,
}
