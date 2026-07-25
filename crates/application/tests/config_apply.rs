use std::path::PathBuf;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use my_supervisor_application::{AppDeps, DaemonMeta, NullProcessServiceRegistrar, OperationsFacade};
use my_supervisor_core::domain::{
    ApplyMode, ChildHandle, ConfigApplyJournal, ConfigApplyStage, ConfigDiff, ConfigSnapshot,
    ConfigTargetDirectStart, DependencyFailurePolicy, Job, JobId, JobRunId, JobTrigger,
    LifecycleMode, LoadedConfig, LogLine, LogRetention, OverlapPolicy, ProcessResourceUsage, ProcessSpec,
};
use my_supervisor_core::ports::error::{ConfigError, RegistrarError, SchedulerError};
use my_supervisor_core::ports::{
    Aliveness, CleanupTicket, ConfigSource, JobRepository, LifecycleController, ProbeError, ReapError, RealClock, StateRepository,
    ProcessServiceRegistrar, ScheduleEvent, ScheduledJob, Scheduler, SchedulerSnapshot, ShutdownSignaler, SignalError,
    SpawnError, TransientCompletion,
};
use my_supervisor_infra_logging::InMemoryLogSink;
use my_supervisor_infra_scheduler::TokioScheduler;
use my_supervisor_infra_sqlite::SqliteStore;
use tokio::sync::{broadcast, watch};

struct NoopLifecycle;

#[async_trait]
impl LifecycleController for NoopLifecycle {
    async fn spawn_tied(&self, _spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> { Err(SpawnError::Io { name: "test".into(), message: "unexpected spawn".into() }) }
    async fn spawn_detached(&self, _spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> { Err(SpawnError::Io { name: "test".into(), message: "unexpected spawn".into() }) }
    async fn probe_alive(&self, _handle: &ChildHandle) -> Result<Aliveness, ProbeError> { Ok(Aliveness::Dead) }
    async fn tail_detached_logs(&self, _spec: &ProcessSpec, _lines: usize, _since: Option<chrono::DateTime<chrono::Utc>>, _after_sequence: Option<u64>, _known_process_names: &[String]) -> Result<my_supervisor_core::ports::LogTail, ProbeError> { Ok(my_supervisor_core::ports::LogTail::default()) }
    async fn subscribe_detached_logs(&self, _spec: &ProcessSpec) -> Result<broadcast::Receiver<LogLine>, ProbeError> { Err(ProbeError::Failed("unexpected log subscribe".into())) }
    async fn resource_usage(&self, _handle: &ChildHandle) -> Result<ProcessResourceUsage, ProbeError> { Ok(ProcessResourceUsage::default()) }
    async fn reap_on_shutdown(&self, _handles: &[ChildHandle]) -> Result<(), ReapError> { Ok(()) }
    async fn start_transient(&self, _spec: &ProcessSpec, _run_id: JobRunId) -> Result<ChildHandle, SpawnError> { Err(SpawnError::Io { name: "test".into(), message: "unexpected run".into() }) }
    async fn complete_transient(&self, _handle: &ChildHandle, _timeout: Option<std::time::Duration>, _cancellation: &mut watch::Receiver<bool>) -> Result<TransientCompletion, SpawnError> { unreachable!() }
    async fn resume_transient_cleanup(&self, _ticket: &CleanupTicket) -> Result<TransientCompletion, SpawnError> { unreachable!() }
}

struct TrackingLifecycle {
    next_pid: AtomicU32,
    failed_spawns: AtomicUsize,
    spawned: Mutex<Vec<ProcessSpec>>,
    alive: Mutex<HashMap<uuid::Uuid, ChildHandle>>,
}

impl TrackingLifecycle {
    fn new() -> Self {
        Self {
            next_pid: AtomicU32::new(10_000),
            failed_spawns: AtomicUsize::new(0),
            spawned: Mutex::new(Vec::new()),
            alive: Mutex::new(HashMap::new()),
        }
    }

    fn fail_next_spawn(&self) {
        self.failed_spawns.fetch_add(1, Ordering::SeqCst);
    }

    fn spawn_count(&self) -> usize {
        self.spawned.lock().unwrap().len()
    }

    fn is_pid_alive(&self, pid: u32) -> bool {
        self.alive.lock().unwrap().values().any(|handle| handle.pid == pid)
    }

    fn stop(&self, handle: &ChildHandle) {
        self.alive.lock().unwrap().remove(&handle.process_id);
    }

    fn spawn(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> {
        if self.failed_spawns.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| remaining.checked_sub(1)).is_ok() {
            return Err(SpawnError::Io { name: spec.name.clone(), message: "injected target start failure".into() });
        }
        let pid = self.next_pid.fetch_add(1, Ordering::SeqCst);
        let handle = ChildHandle {
            process_id: uuid::Uuid::new_v4(),
            pid,
            pgid: Some(pid),
            generation: Some(format!("test-generation-{pid}")),
            started_at: Utc::now(),
        };
        self.spawned.lock().unwrap().push(spec.clone());
        self.alive.lock().unwrap().insert(handle.process_id, handle.clone());
        Ok(handle)
    }
}

struct TrackingShutdown {
    lifecycle: Arc<TrackingLifecycle>,
}

#[async_trait]
impl ShutdownSignaler for TrackingShutdown {
    async fn request_graceful(&self, target: &ChildHandle, _cfg: &my_supervisor_core::domain::ShutdownPolicy) -> Result<(), SignalError> {
        self.lifecycle.stop(target);
        Ok(())
    }

    async fn force_kill(&self, target: &ChildHandle) -> Result<(), SignalError> {
        self.lifecycle.stop(target);
        Ok(())
    }
}

#[async_trait]
impl LifecycleController for TrackingLifecycle {
    async fn spawn_tied(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> { self.spawn(spec) }
    async fn spawn_detached(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> { self.spawn(spec) }
    async fn probe_alive(&self, handle: &ChildHandle) -> Result<Aliveness, ProbeError> {
        Ok(if self.alive.lock().unwrap().contains_key(&handle.process_id) { Aliveness::Alive } else { Aliveness::Dead })
    }
    async fn tail_detached_logs(&self, _spec: &ProcessSpec, _lines: usize, _since: Option<chrono::DateTime<chrono::Utc>>, _after_sequence: Option<u64>, _known_process_names: &[String]) -> Result<my_supervisor_core::ports::LogTail, ProbeError> { Ok(my_supervisor_core::ports::LogTail::default()) }
    async fn subscribe_detached_logs(&self, _spec: &ProcessSpec) -> Result<broadcast::Receiver<LogLine>, ProbeError> { Err(ProbeError::Failed("unexpected log subscribe".into())) }
    async fn resource_usage(&self, _handle: &ChildHandle) -> Result<ProcessResourceUsage, ProbeError> { Ok(ProcessResourceUsage::default()) }
    async fn reap_on_shutdown(&self, _handles: &[ChildHandle]) -> Result<(), ReapError> { Ok(()) }
    async fn start_transient(&self, _spec: &ProcessSpec, _run_id: JobRunId) -> Result<ChildHandle, SpawnError> { unreachable!() }
    async fn complete_transient(&self, _handle: &ChildHandle, _timeout: Option<std::time::Duration>, _cancellation: &mut watch::Receiver<bool>) -> Result<TransientCompletion, SpawnError> { unreachable!() }
    async fn resume_transient_cleanup(&self, _ticket: &CleanupTicket) -> Result<TransientCompletion, SpawnError> { unreachable!() }
}

struct NoopShutdown;

#[async_trait]
impl ShutdownSignaler for NoopShutdown {
    async fn request_graceful(&self, _target: &ChildHandle, _cfg: &my_supervisor_core::domain::ShutdownPolicy) -> Result<(), SignalError> { Ok(()) }
    async fn force_kill(&self, _target: &ChildHandle) -> Result<(), SignalError> { Ok(()) }
}

#[derive(Default)]
struct PreparedRegistrar {
    failing_registration: Mutex<Option<String>>,
    failing_start: AtomicBool,
    running_units: Mutex<std::collections::HashSet<String>>,
    unregistered_units: Mutex<Vec<String>>,
}

impl PreparedRegistrar {
    fn fail_registration_for(&self, unit_name: &str) {
        *self.failing_registration.lock().unwrap() = Some(unit_name.into());
    }

    fn fail_next_start(&self) {
        self.failing_start.store(true, Ordering::SeqCst);
    }

    fn is_running(&self, unit_name: &str) -> bool {
        self.running_units.lock().unwrap().contains(unit_name)
    }

    fn was_unregistered(&self, unit_name: &str) -> bool {
        self.unregistered_units.lock().unwrap().iter().any(|candidate| candidate == unit_name)
    }
}

#[async_trait]
impl ProcessServiceRegistrar for PreparedRegistrar {
    async fn register(&self, unit_name: &str, _spec: &ProcessSpec) -> Result<(), RegistrarError> {
        if self.failing_registration.lock().unwrap().as_deref() == Some(unit_name) {
            return Err(RegistrarError::RegistrationFailed("injected prepare failure".into()));
        }
        Ok(())
    }

    async fn unregister(&self, unit_name: &str) -> Result<(), RegistrarError> {
        self.unregistered_units.lock().unwrap().push(unit_name.into());
        self.running_units.lock().unwrap().remove(unit_name);
        Ok(())
    }

    async fn start(&self, unit_name: &str) -> Result<(), RegistrarError> {
        if self.failing_start.swap(false, Ordering::SeqCst) {
            return Err(RegistrarError::RegistrationFailed("injected start failure".into()));
        }
        self.running_units.lock().unwrap().insert(unit_name.into());
        Ok(())
    }

    async fn stop(&self, unit_name: &str) -> Result<(), RegistrarError> {
        self.running_units.lock().unwrap().remove(unit_name);
        Ok(())
    }

    async fn query_status(&self, _unit_name: &str) -> Result<my_supervisor_core::domain::ProcessState, RegistrarError> {
        Ok(my_supervisor_core::domain::ProcessState::Stopped)
    }

    async fn query_pid(&self, _unit_name: &str) -> Result<Option<u32>, RegistrarError> {
        Ok(None)
    }

    async fn tail_logs(&self, _unit_name: &str, _lines: usize) -> Result<Vec<LogLine>, RegistrarError> {
        Ok(Vec::new())
    }
}

struct EmptyConfig;

#[async_trait]
impl ConfigSource for EmptyConfig {
    async fn load(&self) -> Result<LoadedConfig, ConfigError> { Ok(LoadedConfig::default()) }
    fn path(&self) -> PathBuf { PathBuf::from("test.toml") }
}

struct ControlledScheduler {
    fail_next_register: AtomicBool,
    entries: std::sync::Mutex<HashMap<String, my_supervisor_core::domain::JobTrigger>>,
}

impl ControlledScheduler {
    fn new() -> Self {
        Self { fail_next_register: AtomicBool::new(false), entries: std::sync::Mutex::new(HashMap::new()) }
    }
}

#[async_trait]
impl Scheduler for ControlledScheduler {
    async fn register(&self, name: &str, trigger: &my_supervisor_core::domain::JobTrigger) -> Result<(), SchedulerError> {
        if self.fail_next_register.swap(false, Ordering::SeqCst) { return Err(SchedulerError::Backend("injected scheduler failure".into())); }
        self.entries.lock().unwrap().insert(name.into(), trigger.clone());
        Ok(())
    }
    async fn unregister(&self, name: &str) -> Result<(), SchedulerError> { self.entries.lock().unwrap().remove(name); Ok(()) }
    async fn snapshot(&self) -> Result<SchedulerSnapshot, SchedulerError> {
        let mut entries: Vec<_> = self.entries.lock().unwrap().iter().map(|(name, trigger)| ScheduledJob { name: name.clone(), trigger: trigger.clone() }).collect();
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(SchedulerSnapshot { entries })
    }
    async fn restore(&self, snapshot: &SchedulerSnapshot) -> Result<(), SchedulerError> {
        *self.entries.lock().unwrap() = snapshot.entries.iter().map(|entry| (entry.name.clone(), entry.trigger.clone())).collect();
        Ok(())
    }
    fn next_run(&self, _trigger: &my_supervisor_core::domain::JobTrigger, _after: chrono::DateTime<Utc>) -> Option<chrono::DateTime<Utc>> { None }
    async fn next_event(&self) -> Option<ScheduleEvent> { std::future::pending().await }
}

fn config(name: &str) -> LoadedConfig {
    LoadedConfig { processes: vec![ProcessSpec::new(name, "/bin/true")], jobs: Vec::new() }
}

fn config_dependencies(
    store: Arc<SqliteStore>,
    lifecycle: Arc<dyn LifecycleController>,
) -> AppDeps {
    config_dependencies_with_shutdown(store, lifecycle, Arc::new(NoopShutdown))
}

fn config_dependencies_with_shutdown(
    store: Arc<SqliteStore>,
    lifecycle: Arc<dyn LifecycleController>,
    shutdown: Arc<dyn ShutdownSignaler>,
) -> AppDeps {
    AppDeps {
        lifecycle, shutdown, registrar: Arc::new(NullProcessServiceRegistrar),
        state_repo: store.clone(), job_repo: store, scheduler: Arc::new(TokioScheduler::new()),
        log_sink: Arc::new(InMemoryLogSink::new()), clock: Arc::new(RealClock), config: Arc::new(EmptyConfig),
        meta: DaemonMeta { version: "test".into(), started_at: Utc::now(), pid: 1, config_path: PathBuf::from("test.toml"), log_dir: PathBuf::from("logs") },
    }
}

fn job(name: &str) -> Job {
    Job { id: JobId::new(), name: name.into(), command: "/bin/true".into(), args: Vec::new(), cwd: None,
        env: BTreeMap::new(), trigger: JobTrigger::Interval(std::time::Duration::from_secs(60)),
        on_overlap: OverlapPolicy::Skip, on_dependency_failure: DependencyFailurePolicy::Skip,
        timeout: None, log_retention: LogRetention::default() }
}

#[tokio::test]
async fn dry_run_is_side_effect_free_and_replace_leaves_no_journal() {
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let dependencies = AppDeps {
        lifecycle: Arc::new(NoopLifecycle), shutdown: Arc::new(NoopShutdown),
        registrar: Arc::new(NullProcessServiceRegistrar), state_repo: store.clone(), job_repo: store.clone(),
        scheduler: Arc::new(TokioScheduler::new()), log_sink: Arc::new(InMemoryLogSink::new()),
        clock: Arc::new(RealClock), config: Arc::new(EmptyConfig),
        meta: DaemonMeta { version: "test".into(), started_at: Utc::now(), pid: 1, config_path: PathBuf::from("test.toml"), log_dir: PathBuf::from("logs") },
    };
    let facade = OperationsFacade::new(dependencies);
    facade.apply_config(config("old"), ApplyMode::Merge, false).await.unwrap();
    let dry_run = facade.apply_config(config("new"), ApplyMode::Replace, true).await.unwrap();
    assert!(dry_run.dry_run);
    assert!(facade.get_process("old").await.is_ok());
    assert!(facade.get_process("new").await.is_err());

    facade.apply_config(config("new"), ApplyMode::Replace, false).await.unwrap();
    assert!(facade.get_process("old").await.is_err());
    assert!(facade.get_process("new").await.is_ok());
    assert!(store.list_incomplete_config_applies().await.unwrap().is_empty());
}

#[tokio::test]
async fn prepare_failure_rolls_back_but_post_commit_start_failure_preserves_forward_recovery() {
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let scheduler = Arc::new(ControlledScheduler::new());
    let dependencies = AppDeps {
        lifecycle: Arc::new(NoopLifecycle), shutdown: Arc::new(NoopShutdown), registrar: Arc::new(NullProcessServiceRegistrar),
        state_repo: store.clone(), job_repo: store.clone(), scheduler: scheduler.clone(), log_sink: Arc::new(InMemoryLogSink::new()),
        clock: Arc::new(RealClock), config: Arc::new(EmptyConfig),
        meta: DaemonMeta { version: "test".into(), started_at: Utc::now(), pid: 1, config_path: PathBuf::from("test.toml"), log_dir: PathBuf::from("logs") },
    };
    let facade = OperationsFacade::new(dependencies);
    facade.apply_config(config("old"), ApplyMode::Merge, false).await.unwrap();
    scheduler.fail_next_register.store(true, Ordering::SeqCst);
    let mut scheduler_failure = config("new");
    scheduler_failure.jobs.push(job("new-job"));
    assert!(facade.apply_config(scheduler_failure, ApplyMode::Replace, false).await.is_err());
    assert!(facade.get_process("old").await.is_ok());
    assert!(facade.get_process("new").await.is_err());
    assert!(store.list_incomplete_config_applies().await.unwrap().is_empty());

    let original_job = job("stable");
    facade.add_job(original_job.clone()).await.unwrap();
    let mut changed_job = original_job.clone();
    changed_job.command = "/bin/false".into();
    scheduler.fail_next_register.store(true, Ordering::SeqCst);
    assert!(facade.update_job("stable", changed_job).await.is_err());
    assert_eq!(facade.get_job("stable").await.unwrap().job, original_job);

    let mut start_failure = config("new");
    start_failure.processes[0].autostart = true;
    let error = facade.apply_config(start_failure, ApplyMode::Replace, false).await.unwrap_err();
    assert_eq!(error.code(), "config_recovery_required");
    assert!(facade.get_process("old").await.is_err());
    assert!(facade.get_process("new").await.is_ok());
    let journals = store.list_incomplete_config_applies().await.unwrap();
    assert_eq!(journals.len(), 1);
    assert_eq!(journals[0].stage, my_supervisor_core::domain::ConfigApplyStage::ForwardRecovery);
}

#[tokio::test]
async fn same_label_prepare_failure_preserves_old_unit_and_start_failure_is_forward_recovery() {
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let registrar = Arc::new(PreparedRegistrar::default());
    let dependencies = AppDeps {
        lifecycle: Arc::new(NoopLifecycle), shutdown: Arc::new(NoopShutdown), registrar: registrar.clone(),
        state_repo: store.clone(), job_repo: store.clone(), scheduler: Arc::new(TokioScheduler::new()), log_sink: Arc::new(InMemoryLogSink::new()),
        clock: Arc::new(RealClock), config: Arc::new(EmptyConfig),
        meta: DaemonMeta { version: "test".into(), started_at: Utc::now(), pid: 1, config_path: PathBuf::from("test.toml"), log_dir: PathBuf::from("logs") },
    };
    let facade = OperationsFacade::new(dependencies);
    let label = "com.example.same-label";
    let mut old = ProcessSpec::new("service", "/bin/old");
    old.autostart = true;
    old.management_mode = my_supervisor_core::domain::ManagementMode::SystemRegistered { unit_name: label.into() };
    facade.apply_config(LoadedConfig { processes: vec![old.clone()], jobs: Vec::new() }, ApplyMode::Replace, false).await.unwrap();
    assert!(registrar.is_running(label));

    let mut changed = old.clone();
    changed.command = "/bin/new".into();
    let mut failing_prepare = ProcessSpec::new("second", "/bin/second");
    failing_prepare.management_mode = my_supervisor_core::domain::ManagementMode::SystemRegistered { unit_name: "com.example.prepare-failure".into() };
    registrar.fail_registration_for("com.example.prepare-failure");
    assert!(facade.apply_config(LoadedConfig { processes: vec![changed.clone(), failing_prepare], jobs: Vec::new() }, ApplyMode::Replace, false).await.is_err());
    assert!(registrar.is_running(label), "same-label prepare failure must not stop the old unit");
    assert!(!registrar.was_unregistered(label));
    assert!(store.list_incomplete_config_applies().await.unwrap().is_empty());

    *registrar.failing_registration.lock().unwrap() = None;
    registrar.fail_next_start();
    let error = facade.apply_config(LoadedConfig { processes: vec![changed], jobs: Vec::new() }, ApplyMode::Replace, false).await.unwrap_err();
    assert_eq!(error.code(), "config_recovery_required");
    assert!(registrar.is_running(label), "same-label live unit remains until a successful replacement start");
    assert!(!registrar.was_unregistered(label));
    assert_eq!(store.list_incomplete_config_applies().await.unwrap()[0].stage, ConfigApplyStage::ForwardRecovery);
}

#[tokio::test]
async fn forward_recovery_restarts_previously_running_autostart_false_target_after_start_failure() {
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let first_lifecycle = Arc::new(TrackingLifecycle::new());
    let first_facade = OperationsFacade::new(config_dependencies_with_shutdown(
        store.clone(),
        first_lifecycle.clone(),
        Arc::new(TrackingShutdown { lifecycle: first_lifecycle.clone() }),
    ));
    let mut old = ProcessSpec::new("worker", "/bin/old");
    old.autostart = false;
    first_facade.apply_config(LoadedConfig { processes: vec![old], jobs: Vec::new() }, ApplyMode::Replace, false).await.unwrap();
    first_facade.start_process("worker").await.unwrap();
    let old_pid = first_facade.get_process("worker").await.unwrap().pid.unwrap();

    let mut target = ProcessSpec::new("worker", "/bin/new");
    target.autostart = false;
    first_lifecycle.fail_next_spawn();
    let apply_error = first_facade.apply_config(
        LoadedConfig { processes: vec![target.clone()], jobs: Vec::new() },
        ApplyMode::Replace,
        false,
    ).await.unwrap_err();
    assert_eq!(apply_error.code(), "config_recovery_required");
    assert!(!first_lifecycle.is_pid_alive(old_pid), "the old target PID must be reaped before forward recovery");
    let journal = store.list_incomplete_config_applies().await.unwrap().pop().unwrap();
    assert_eq!(journal.previous.running_direct_processes, vec!["worker"]);
    assert_eq!(journal.target_direct_starts.len(), 1);
    assert_eq!(journal.target_direct_starts[0].expected_generation, None);

    let recovered_lifecycle = Arc::new(TrackingLifecycle::new());
    let recovered_facade = OperationsFacade::new(config_dependencies(store.clone(), recovered_lifecycle.clone()));
    recovered_facade.recover_incomplete_config_apply().await.unwrap();

    assert_eq!(recovered_lifecycle.spawn_count(), 1, "the target must start once even with autostart=false");
    assert_eq!(recovered_facade.get_process("worker").await.unwrap().state, my_supervisor_core::domain::ProcessState::Running);
    assert!(store.list_incomplete_config_applies().await.unwrap().is_empty());
}

#[tokio::test]
async fn forward_recovery_recognizes_generation_recorded_before_crash_without_duplicate_spawn() {
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let lifecycle = Arc::new(TrackingLifecycle::new());
    let mut target = ProcessSpec::new("worker", "/bin/new");
    target.lifecycle = LifecycleMode::Detached;
    target.autostart = false;
    store.apply_config_snapshot(&ConfigSnapshot {
        processes: vec![target.clone()], jobs: Vec::new(), running_direct_processes: Vec::new(),
    }).await.unwrap();
    let handle = lifecycle.spawn_detached(&target).await.unwrap();
    store.set_runtime_handle(&target.name, Some(&handle)).await.unwrap();
    let apply_id = uuid::Uuid::new_v4();
    store.create_config_apply_journal(&ConfigApplyJournal {
        apply_id,
        previous: ConfigSnapshot {
            processes: vec![target.clone()], jobs: Vec::new(), running_direct_processes: vec![target.name.clone()],
        },
        target: ConfigSnapshot {
            processes: vec![target.clone()], jobs: Vec::new(), running_direct_processes: vec![target.name.clone()],
        },
        diff: ConfigDiff::default(),
        stage: ConfigApplyStage::ForwardRecovery,
        compensation_error: Some("simulated crash after target start".into()),
        target_direct_starts: vec![ConfigTargetDirectStart {
            name: target.name.clone(), spec: target.clone(), expected_generation: handle.generation.clone(),
        }],
    }).await.unwrap();

    let recovered_facade = OperationsFacade::new(config_dependencies(store.clone(), lifecycle.clone()));
    recovered_facade.recover_incomplete_config_apply().await.unwrap();

    assert_eq!(lifecycle.spawn_count(), 1, "recovery must restore the verified target instead of spawning another");
    assert_eq!(recovered_facade.get_process("worker").await.unwrap().pid, Some(handle.pid));
    assert!(store.list_incomplete_config_applies().await.unwrap().is_empty());
}

#[tokio::test]
async fn replace_removes_a_previously_running_direct_process_from_the_runtime_target() {
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let lifecycle = Arc::new(TrackingLifecycle::new());
    let facade = OperationsFacade::new(config_dependencies_with_shutdown(
        store.clone(),
        lifecycle.clone(),
        Arc::new(TrackingShutdown { lifecycle: lifecycle.clone() }),
    ));
    let mut removed = ProcessSpec::new("removed", "/bin/old");
    removed.autostart = false;
    facade.apply_config(LoadedConfig { processes: vec![removed], jobs: Vec::new() }, ApplyMode::Replace, false).await.unwrap();
    facade.start_process("removed").await.unwrap();
    let old_pid = facade.get_process("removed").await.unwrap().pid.unwrap();

    facade.apply_config(LoadedConfig::default(), ApplyMode::Replace, false).await.unwrap();

    assert!(!lifecycle.is_pid_alive(old_pid), "a removed target must not retain its old PID");
    assert!(facade.get_process("removed").await.is_err());
    assert!(store.list_incomplete_config_applies().await.unwrap().is_empty());
}
