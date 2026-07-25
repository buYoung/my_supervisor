use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::broadcast;
use tokio::sync::watch;
use uuid::Uuid;

use my_supervisor_application::{
    AppDeps, DaemonMeta, DomainEvent, NullProcessServiceRegistrar, OperationsFacade,
};
use my_supervisor_core::domain::{
    ChildHandle, Job, JobDeletionJournal, JobDeletionStage, JobId, JobRun, JobRunId, JobRunState, JobTrigger, LifecycleMode, LoadedConfig, LogLine,
    LogRetention, LogStream, ManagementMode, OverlapPolicy, ProcessResourceUsage, ProcessSpec,
    ProcessState, TriggeredBy,
};
use my_supervisor_core::ports::error::RegistrarError;
use my_supervisor_core::ports::{
    Aliveness, CleanupTicket, ConfigError, ConfigSource, LifecycleController, ProbeError, RealClock, ReapError,
    JobRepository, LogSink, ProcessServiceRegistrar, ShutdownSignaler, SignalError, SpawnError, StateRepository, TransientCleanupStage, TransientCompletion,
    TransientOutcome,
};
use my_supervisor_core::ports::error::RepoError;
use my_supervisor_infra_logging::InMemoryLogSink;
use my_supervisor_infra_scheduler::TokioScheduler;
use my_supervisor_infra_sqlite::SqliteStore;

struct EmptyConfig;

#[async_trait]
impl ConfigSource for EmptyConfig {
    async fn load(&self) -> Result<LoadedConfig, ConfigError> {
        Ok(LoadedConfig::default())
    }

    fn path(&self) -> PathBuf {
        PathBuf::from("test.toml")
    }
}

struct FixedConfig {
    loaded: LoadedConfig,
}

#[async_trait]
impl ConfigSource for FixedConfig {
    async fn load(&self) -> Result<LoadedConfig, ConfigError> {
        Ok(self.loaded.clone())
    }

    fn path(&self) -> PathBuf {
        PathBuf::from("test.toml")
    }
}

struct FakeLifecycle {
    next_pid: AtomicU32,
    alive: Mutex<HashMap<Uuid, bool>>,
    spawn_count: AtomicUsize,
    detached_subscription_count: AtomicUsize,
    transient_delay: Duration,
    concurrent_runs: AtomicUsize,
    max_concurrent_runs: AtomicUsize,
    transient_started: AtomicUsize,
    leave_unreaped_on_cancel: std::sync::atomic::AtomicBool,
}

impl FakeLifecycle {
    fn new(transient_delay: Duration) -> Self {
        Self {
            next_pid: AtomicU32::new(1000),
            alive: Mutex::new(HashMap::new()),
            spawn_count: AtomicUsize::new(0),
            detached_subscription_count: AtomicUsize::new(0),
            transient_delay,
            concurrent_runs: AtomicUsize::new(0),
            max_concurrent_runs: AtomicUsize::new(0),
            transient_started: AtomicUsize::new(0),
            leave_unreaped_on_cancel: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn spawn(&self) -> ChildHandle {
        let process_id = Uuid::new_v4();
        self.alive.lock().unwrap().insert(process_id, true);
        self.spawn_count.fetch_add(1, Ordering::SeqCst);
        ChildHandle {
            process_id,
            pid: self.next_pid.fetch_add(1, Ordering::SeqCst),
            pgid: Some(self.next_pid.load(Ordering::SeqCst) - 1),
            generation: Some("fake-generation".to_string()),
            started_at: Utc::now(),
        }
    }

    fn mark_all_dead(&self) {
        for is_alive in self.alive.lock().unwrap().values_mut() {
            *is_alive = false;
        }
    }

    fn finish(&self, handle: &ChildHandle) {
        self.alive
            .lock()
            .unwrap()
            .insert(handle.process_id, false);
    }

    fn leave_unreaped_on_cancel(&self) {
        self.leave_unreaped_on_cancel.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl LifecycleController for FakeLifecycle {
    async fn spawn_tied(&self, _spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> {
        Ok(self.spawn())
    }

    async fn spawn_detached(&self, _spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> {
        Ok(self.spawn())
    }

    async fn probe_alive(&self, handle: &ChildHandle) -> Result<Aliveness, ProbeError> {
        Ok(if self
            .alive
            .lock()
            .unwrap()
            .get(&handle.process_id)
            .copied()
            .unwrap_or(false)
        {
            Aliveness::Alive
        } else {
            Aliveness::Dead
        })
    }

    async fn tail_detached_logs(
        &self,
        _spec: &ProcessSpec,
        _lines: usize,
        _since: Option<chrono::DateTime<chrono::Utc>>,
        _after_sequence: Option<u64>,
        _known_process_names: &[String],
    ) -> Result<my_supervisor_core::ports::LogTail, ProbeError> {
        Ok(my_supervisor_core::ports::LogTail::default())
    }

    async fn subscribe_detached_logs(
        &self,
        _spec: &ProcessSpec,
    ) -> Result<broadcast::Receiver<LogLine>, ProbeError> {
        self.detached_subscription_count
            .fetch_add(1, Ordering::SeqCst);
        let (_sender, receiver) = broadcast::channel(8);
        Ok(receiver)
    }

    async fn resource_usage(&self, _handle: &ChildHandle) -> Result<ProcessResourceUsage, ProbeError> {
        Ok(ProcessResourceUsage {
            cpu_percent: 12.5,
            memory_bytes: 8 * 1024 * 1024,
        })
    }

    async fn reap_on_shutdown(&self, handles: &[ChildHandle]) -> Result<(), ReapError> {
        for handle in handles {
            self.finish(handle);
        }
        Ok(())
    }

    async fn start_transient(
        &self,
        _spec: &ProcessSpec,
        _run_id: JobRunId,
    ) -> Result<ChildHandle, SpawnError> {
        Ok(self.spawn())
    }

    async fn complete_transient(
        &self,
        _handle: &ChildHandle,
        timeout: Option<Duration>,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<TransientCompletion, SpawnError> {
        let started_at = Utc::now();
        self.transient_started.fetch_add(1, Ordering::SeqCst);
        let concurrent = self.concurrent_runs.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_concurrent_runs
            .fetch_max(concurrent, Ordering::SeqCst);
        enum CompletionCause {
            Exited,
            TimedOut,
            Cancelled,
        }
        let cancellation_requested = async {
            if !*cancellation.borrow() {
                let _ = cancellation.changed().await;
            }
        };
        let completion_cause = tokio::select! {
            _ = tokio::time::sleep(self.transient_delay) => CompletionCause::Exited,
            _ = cancellation_requested => CompletionCause::Cancelled,
            _ = tokio::time::sleep(timeout.unwrap_or_default()), if timeout.is_some() => CompletionCause::TimedOut,
        };
        self.concurrent_runs.fetch_sub(1, Ordering::SeqCst);
        let outcome = TransientOutcome {
            started_at,
            ended_at: Utc::now(),
            exit_code: Some(0),
        };
        Ok(match completion_cause {
            CompletionCause::Exited => TransientCompletion::Exited(outcome),
            CompletionCause::TimedOut => TransientCompletion::TimedOut(outcome),
            CompletionCause::Cancelled if self.leave_unreaped_on_cancel.load(Ordering::SeqCst) => {
                TransientCompletion::CleanupPending {
                    cause: "injected cleanup failure".into(),
                    stage: TransientCleanupStage::JoinPumps,
                    intended_terminal_state: my_supervisor_core::domain::JobRunState::Cancelled,
                    outcome,
                }
            }
            CompletionCause::Cancelled => TransientCompletion::Cancelled(outcome),
        })
    }

    async fn resume_transient_cleanup(
        &self,
        ticket: &CleanupTicket,
    ) -> Result<TransientCompletion, SpawnError> {
        self.finish(&ticket.child);
        Ok(TransientCompletion::Cancelled(TransientOutcome {
            started_at: ticket.child.started_at,
            ended_at: Utc::now(),
            exit_code: Some(0),
        }))
    }
}

#[derive(Default)]
struct FakeRegistrar {
    is_running: Mutex<bool>,
}

#[async_trait]
impl ProcessServiceRegistrar for FakeRegistrar {
    async fn register(&self, _unit_name: &str, _spec: &ProcessSpec) -> Result<(), RegistrarError> {
        Ok(())
    }

    async fn unregister(&self, _unit_name: &str) -> Result<(), RegistrarError> {
        *self.is_running.lock().unwrap() = false;
        Ok(())
    }

    async fn start(&self, _unit_name: &str) -> Result<(), RegistrarError> {
        *self.is_running.lock().unwrap() = true;
        Ok(())
    }

    async fn stop(&self, _unit_name: &str) -> Result<(), RegistrarError> {
        *self.is_running.lock().unwrap() = false;
        Ok(())
    }

    async fn query_status(&self, _unit_name: &str) -> Result<ProcessState, RegistrarError> {
        Ok(if *self.is_running.lock().unwrap() {
            ProcessState::Running
        } else {
            ProcessState::Stopped
        })
    }

    async fn query_pid(&self, _unit_name: &str) -> Result<Option<u32>, RegistrarError> {
        Ok((*self.is_running.lock().unwrap()).then_some(4242))
    }

    async fn tail_logs(
        &self,
        _unit_name: &str,
        _lines: usize,
    ) -> Result<Vec<LogLine>, RegistrarError> {
        Ok(Vec::new())
    }
}

struct FakeShutdown {
    lifecycle: Arc<FakeLifecycle>,
}

struct FailFirstRuntimeHandleClear {
    inner: Arc<SqliteStore>,
    should_fail_clear: std::sync::atomic::AtomicBool,
}

impl FailFirstRuntimeHandleClear {
    fn fail_next_clear(&self) {
        self.should_fail_clear.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl StateRepository for FailFirstRuntimeHandleClear {
    async fn list_specs(&self) -> Result<Vec<ProcessSpec>, RepoError> {
        self.inner.list_specs().await
    }

    async fn get_spec(&self, name: &str) -> Result<Option<ProcessSpec>, RepoError> {
        self.inner.get_spec(name).await
    }

    async fn save_spec(&self, spec: &ProcessSpec) -> Result<(), RepoError> {
        self.inner.save_spec(spec).await
    }

    async fn delete_spec(&self, name: &str) -> Result<(), RepoError> {
        self.inner.delete_spec(name).await
    }

    async fn get_restart_count(&self, name: &str) -> Result<u32, RepoError> {
        self.inner.get_restart_count(name).await
    }

    async fn set_restart_count(&self, name: &str, count: u32) -> Result<(), RepoError> {
        self.inner.set_restart_count(name, count).await
    }

    async fn get_runtime_handle(&self, name: &str) -> Result<Option<ChildHandle>, RepoError> {
        self.inner.get_runtime_handle(name).await
    }

    async fn set_runtime_handle(
        &self,
        name: &str,
        handle: Option<&ChildHandle>,
    ) -> Result<(), RepoError> {
        if handle.is_none()
            && self
                .should_fail_clear
                .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            return Err(RepoError::Backend("injected runtime-handle clear failure".into()));
        }
        self.inner.set_runtime_handle(name, handle).await
    }

    async fn enqueue_runtime_handle_cleanup(
        &self,
        name: &str,
        handle: &ChildHandle,
        error: &str,
    ) -> Result<(), RepoError> {
        self.inner.enqueue_runtime_handle_cleanup(name, handle, error).await
    }

    async fn pending_runtime_handle_cleanup(
        &self,
        limit: usize,
    ) -> Result<Vec<my_supervisor_core::domain::process::RuntimeHandleCleanup>, RepoError> {
        self.inner.pending_runtime_handle_cleanup(limit).await
    }

    async fn clear_runtime_handle_if_matches(
        &self,
        cleanup: &my_supervisor_core::domain::process::RuntimeHandleCleanup,
    ) -> Result<bool, RepoError> {
        self.inner.clear_runtime_handle_if_matches(cleanup).await
    }

    async fn complete_runtime_handle_cleanup(&self, name: &str) -> Result<(), RepoError> {
        self.inner.complete_runtime_handle_cleanup(name).await
    }
}

#[async_trait]
impl ShutdownSignaler for FakeShutdown {
    async fn request_graceful(
        &self,
        target: &ChildHandle,
        _policy: &my_supervisor_core::domain::ShutdownPolicy,
    ) -> Result<(), SignalError> {
        self.lifecycle.finish(target);
        Ok(())
    }

    async fn force_kill(&self, target: &ChildHandle) -> Result<(), SignalError> {
        self.lifecycle.finish(target);
        Ok(())
    }
}

async fn facade(transient_delay: Duration) -> (Arc<OperationsFacade>, Arc<FakeLifecycle>) {
    let lifecycle = Arc::new(FakeLifecycle::new(transient_delay));
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let facade = facade_with_store(lifecycle.clone(), store).await;
    (facade, lifecycle)
}

async fn facade_with_store(
    lifecycle: Arc<FakeLifecycle>,
    store: Arc<SqliteStore>,
) -> Arc<OperationsFacade> {
    facade_with_store_and_config(lifecycle, store, Arc::new(EmptyConfig)).await
}

async fn facade_with_store_and_config(
    lifecycle: Arc<FakeLifecycle>,
    store: Arc<SqliteStore>,
    config: Arc<dyn ConfigSource>,
) -> Arc<OperationsFacade> {
    let log_sink = Arc::new(InMemoryLogSink::new());
    let dependencies = AppDeps {
        lifecycle: lifecycle.clone(),
        shutdown: Arc::new(FakeShutdown {
            lifecycle: lifecycle.clone(),
        }),
        registrar: Arc::new(NullProcessServiceRegistrar),
        state_repo: store.clone(),
        job_repo: store,
        scheduler: Arc::new(TokioScheduler::new()),
        log_sink,
        clock: Arc::new(RealClock),
        config,
        meta: DaemonMeta::new(PathBuf::from("test.toml"), PathBuf::from("logs")),
    };
    OperationsFacade::new(dependencies)
}

async fn facade_with_first_runtime_handle_clear_failure(
) -> (Arc<OperationsFacade>, Arc<FailFirstRuntimeHandleClear>) {
    let lifecycle = Arc::new(FakeLifecycle::new(Duration::from_millis(10)));
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let state_repo = Arc::new(FailFirstRuntimeHandleClear {
        inner: store.clone(),
        should_fail_clear: std::sync::atomic::AtomicBool::new(false),
    });
    let dependencies = AppDeps {
        lifecycle: lifecycle.clone(),
        shutdown: Arc::new(FakeShutdown { lifecycle }),
        registrar: Arc::new(NullProcessServiceRegistrar),
        state_repo: state_repo.clone(),
        job_repo: store,
        scheduler: Arc::new(TokioScheduler::new()),
        log_sink: Arc::new(InMemoryLogSink::new()),
        clock: Arc::new(RealClock),
        config: Arc::new(EmptyConfig),
        meta: DaemonMeta::new(PathBuf::from("test.toml"), PathBuf::from("logs")),
    };
    (OperationsFacade::new(dependencies), state_repo)
}

#[tokio::test]
async fn direct_stop_emits_stopped_once_when_handle_cleanup_retries() {
    let (facade, state_repo) = facade_with_first_runtime_handle_clear_failure().await;
    let mut events = facade.subscribe_events();
    let mut spec = ProcessSpec::new("retry-stop", "fake");
    spec.lifecycle = LifecycleMode::Detached;
    facade.add_process(spec).await.unwrap();
    facade.start_process("retry-stop").await.unwrap();
    assert!(matches!(events.recv().await.unwrap().event, DomainEvent::ProcessStateChanged {
        name,
        from: ProcessState::Stopped,
        to: ProcessState::Running,
    } if name == "retry-stop"));

    state_repo.fail_next_clear();
    assert!(facade.stop_process("retry-stop", false).await.is_err());
    assert!(matches!(events.recv().await.unwrap().event, DomainEvent::ProcessStateChanged {
        name,
        from: ProcessState::Running,
        to: ProcessState::Stopped,
    } if name == "retry-stop"));
    assert_eq!(state_repo.pending_runtime_handle_cleanup(10).await.unwrap().len(), 1);

    facade.bootstrap().await.unwrap();
    assert!(state_repo.pending_runtime_handle_cleanup(10).await.unwrap().is_empty());
    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(20), events.recv()).await,
        Err(_)
    ));
}

async fn facade_with_registrar(
    transient_delay: Duration,
    registrar: Arc<dyn ProcessServiceRegistrar>,
) -> (Arc<OperationsFacade>, Arc<FakeLifecycle>) {
    let lifecycle = Arc::new(FakeLifecycle::new(transient_delay));
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let dependencies = AppDeps {
        lifecycle: lifecycle.clone(),
        shutdown: Arc::new(FakeShutdown {
            lifecycle: lifecycle.clone(),
        }),
        registrar,
        state_repo: store.clone(),
        job_repo: store,
        scheduler: Arc::new(TokioScheduler::new()),
        log_sink: Arc::new(InMemoryLogSink::new()),
        clock: Arc::new(RealClock),
        config: Arc::new(EmptyConfig),
        meta: DaemonMeta::new(PathBuf::from("test.toml"), PathBuf::from("logs")),
    };
    (OperationsFacade::new(dependencies), lifecycle)
}

async fn facade_with_log_sink(
    log_sink: Arc<InMemoryLogSink>,
) -> (Arc<OperationsFacade>, Arc<FakeLifecycle>) {
    let lifecycle = Arc::new(FakeLifecycle::new(Duration::from_millis(10)));
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let dependencies = AppDeps {
        lifecycle: lifecycle.clone(),
        shutdown: Arc::new(FakeShutdown {
            lifecycle: lifecycle.clone(),
        }),
        registrar: Arc::new(NullProcessServiceRegistrar),
        state_repo: store.clone(),
        job_repo: store,
        scheduler: Arc::new(TokioScheduler::new()),
        log_sink,
        clock: Arc::new(RealClock),
        config: Arc::new(EmptyConfig),
        meta: DaemonMeta::new(PathBuf::from("test.toml"), PathBuf::from("logs")),
    };
    (OperationsFacade::new(dependencies), lifecycle)
}

fn job(name: &str, overlap: OverlapPolicy) -> Job {
    Job {
        id: JobId::new(),
        name: name.to_string(),
        command: "fake".to_string(),
        args: Vec::new(),
        cwd: None,
        env: BTreeMap::new(),
        trigger: JobTrigger::Interval(Duration::from_secs(3600)),
        on_overlap: overlap,
        on_dependency_failure: Default::default(),
        timeout: None,
        log_retention: LogRetention::default(),
    }
}

#[tokio::test]
async fn queued_runs_are_serial_and_parallel_runs_overlap() {
    let (queue_facade, queue_lifecycle) = facade(Duration::from_millis(50)).await;
    queue_facade
        .add_job(job("queue", OverlapPolicy::Queue))
        .await
        .unwrap();
    for _ in 0..3 {
        queue_facade.trigger_job("queue").await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(220)).await;
    assert_eq!(queue_lifecycle.max_concurrent_runs.load(Ordering::SeqCst), 1);
    assert!(queue_facade
        .list_runs("queue", 3)
        .await
        .unwrap()
        .iter()
        .all(|run| run.state.is_terminal()));

    let (parallel_facade, parallel_lifecycle) = facade(Duration::from_millis(50)).await;
    parallel_facade
        .add_job(job("parallel", OverlapPolicy::Parallel))
        .await
        .unwrap();
    for _ in 0..3 {
        parallel_facade.trigger_job("parallel").await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        parallel_lifecycle.max_concurrent_runs.load(Ordering::SeqCst),
        3
    );
}

#[tokio::test]
async fn queued_cancellation_never_starts_the_child_and_persists_cancelled() {
    let (facade, lifecycle) = facade(Duration::from_millis(300)).await;
    facade.add_job(job("queued-cancel", OverlapPolicy::Queue)).await.unwrap();
    facade.trigger_job("queued-cancel").await.unwrap();
    for _ in 0..20 {
        if lifecycle.transient_started.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let queued_run_id = facade.trigger_job("queued-cancel").await.unwrap();
    facade.cancel_run("queued-cancel", queued_run_id).await.unwrap();

    let queued_run = facade.get_run("queued-cancel", &queued_run_id).await.unwrap();
    assert_eq!(queued_run.state, my_supervisor_core::domain::JobRunState::Cancelled);
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(lifecycle.transient_started.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn active_cancellation_waits_for_controlled_completion() {
    let (facade, lifecycle) = facade(Duration::from_secs(2)).await;
    facade.add_job(job("active-cancel", OverlapPolicy::Skip)).await.unwrap();
    let run_id = facade.trigger_job("active-cancel").await.unwrap();
    for _ in 0..20 {
        if lifecycle.transient_started.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    facade.cancel_run("active-cancel", run_id).await.unwrap();
    for _ in 0..20 {
        let run = facade.get_run("active-cancel", &run_id).await.unwrap();
        if run.state == my_supervisor_core::domain::JobRunState::Cancelled {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        facade.get_run("active-cancel", &run_id).await.unwrap().state,
        my_supervisor_core::domain::JobRunState::Cancelled
    );
    assert_eq!(lifecycle.concurrent_runs.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn parallel_runs_have_independent_cancellation() {
    let (facade, lifecycle) = facade(Duration::from_millis(180)).await;
    facade.add_job(job("parallel-cancel", OverlapPolicy::Parallel)).await.unwrap();
    let cancelled_run_id = facade.trigger_job("parallel-cancel").await.unwrap();
    let surviving_run_id = facade.trigger_job("parallel-cancel").await.unwrap();
    for _ in 0..20 {
        if lifecycle.transient_started.load(Ordering::SeqCst) == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    facade.cancel_run("parallel-cancel", cancelled_run_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        facade.get_run("parallel-cancel", &cancelled_run_id).await.unwrap().state,
        my_supervisor_core::domain::JobRunState::Cancelled
    );
    assert_eq!(
        facade.get_run("parallel-cancel", &surviving_run_id).await.unwrap().state,
        my_supervisor_core::domain::JobRunState::Succeeded
    );
}

#[tokio::test]
async fn cancelling_a_run_through_another_job_is_a_404_without_mutation() {
    let (facade, lifecycle) = facade(Duration::from_secs(2)).await;
    facade.add_job(job("job-a", OverlapPolicy::Skip)).await.unwrap();
    facade.add_job(job("job-b", OverlapPolicy::Queue)).await.unwrap();
    let active_run_id = facade.trigger_job("job-b").await.unwrap();
    for _ in 0..20 {
        if lifecycle.transient_started.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let queued_run_id = facade.trigger_job("job-b").await.unwrap();

    let active_error = facade.cancel_run("job-a", active_run_id).await.unwrap_err();
    let queued_error = facade.cancel_run("job-a", queued_run_id).await.unwrap_err();
    assert_eq!(active_error.code(), "run_not_found");
    assert_eq!(queued_error.code(), "run_not_found");
    assert_eq!(
        facade.get_run("job-b", &active_run_id).await.unwrap().state,
        my_supervisor_core::domain::JobRunState::Running
    );
    assert_eq!(
        facade.get_run("job-b", &queued_run_id).await.unwrap().state,
        my_supervisor_core::domain::JobRunState::Pending
    );

    facade.cancel_run("job-b", active_run_id).await.unwrap();
}

#[tokio::test]
async fn tombstoned_job_cannot_attach_a_late_completion_to_a_reused_name() {
    let (facade, lifecycle) = facade(Duration::from_secs(2)).await;
    facade.add_job(job("reused-name", OverlapPolicy::Skip)).await.unwrap();
    facade.trigger_job("reused-name").await.unwrap();
    for _ in 0..20 {
        if lifecycle.transient_started.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    facade.delete_job("reused-name", true).await.unwrap();
    facade.add_job(job("reused-name", OverlapPolicy::Skip)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(facade.list_runs("reused-name", 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn non_force_delete_rejects_an_active_run_without_mutating_the_job() {
    let (facade, lifecycle) = facade(Duration::from_secs(2)).await;
    facade.add_job(job("delete-active", OverlapPolicy::Skip)).await.unwrap();
    let run_id = facade.trigger_job("delete-active").await.unwrap();
    for _ in 0..20 {
        if lifecycle.transient_started.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let error = facade.delete_job("delete-active", false).await.unwrap_err();
    assert_eq!(error.code(), "job_has_active_runs");
    assert_eq!(
        facade.get_run("delete-active", &run_id).await.unwrap().state,
        my_supervisor_core::domain::JobRunState::Running
    );
}

#[tokio::test]
async fn force_delete_waits_for_parallel_run_drain_before_removing_the_job() {
    let (facade, lifecycle) = facade(Duration::from_secs(2)).await;
    facade.add_job(job("force-drain", OverlapPolicy::Parallel)).await.unwrap();
    facade.trigger_job("force-drain").await.unwrap();
    facade.trigger_job("force-drain").await.unwrap();
    for _ in 0..20 {
        if lifecycle.transient_started.load(Ordering::SeqCst) == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    facade.delete_job("force-drain", true).await.unwrap();
    assert_eq!(lifecycle.concurrent_runs.load(Ordering::SeqCst), 0);
    assert!(facade.get_job("force-drain").await.is_err());
}

#[tokio::test]
async fn shutdown_drains_queued_and_active_runs_before_reaping_children() {
    let (facade, lifecycle) = facade(Duration::from_secs(2)).await;
    facade.add_job(job("shutdown-drain", OverlapPolicy::Queue)).await.unwrap();
    let active_run = facade.trigger_job("shutdown-drain").await.unwrap();
    let queued_run = facade.trigger_job("shutdown-drain").await.unwrap();
    for _ in 0..20 {
        if lifecycle.transient_started.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    facade.shutdown_all().await.unwrap();
    assert_eq!(lifecycle.concurrent_runs.load(Ordering::SeqCst), 0);
    assert_eq!(
        facade.get_run("shutdown-drain", &active_run).await.unwrap().state,
        my_supervisor_core::domain::JobRunState::Cancelled
    );
    assert_eq!(
        facade.get_run("shutdown-drain", &queued_run).await.unwrap().state,
        my_supervisor_core::domain::JobRunState::Cancelled
    );
}

#[tokio::test]
async fn cleanup_ticket_retries_with_the_same_event_id_after_ack_failure() {
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let lifecycle = Arc::new(FakeLifecycle::new(Duration::from_secs(2)));
    let facade = facade_with_store(lifecycle.clone(), store.clone()).await;
    let mut events = facade.subscribe_events();
    lifecycle.leave_unreaped_on_cancel();
    facade.add_job(job("unreaped", OverlapPolicy::Skip)).await.unwrap();
    let run_id = facade.trigger_job("unreaped").await.unwrap();
    for _ in 0..20 {
        if lifecycle.transient_started.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    facade.cancel_run("unreaped", run_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        facade.get_run("unreaped", &run_id).await.unwrap().state,
        my_supervisor_core::domain::JobRunState::Running
    );
    store.fail_next_transient_terminal_acknowledgements(1);
    let first_shutdown_facade = facade.clone();
    let first_shutdown = tokio::spawn(async move { first_shutdown_facade.shutdown_all().await });
    let mut cancelled_events = 0;
    let first_event_id = loop {
        let event = events.recv().await.unwrap();
        if matches!(&event.event, DomainEvent::JobRunCancelled { run_id: event_run_id, .. } if event_run_id == &run_id) {
            cancelled_events += 1;
        }
        if let Some(event_id) = event.event_id {
            event.complete_delivery();
            break event_id;
        }
    };
    assert!(first_shutdown.await.unwrap().is_err());
    assert_eq!(store.pending_transient_cleanup(10).await.unwrap().len(), 1);
    assert_eq!(store.pending_transient_terminal_events(10).await.unwrap().len(), 1);

    let restarted_facade = facade_with_store_and_config(
        Arc::new(FakeLifecycle::new(Duration::ZERO)),
        store.clone(),
        Arc::new(FixedConfig {
            loaded: LoadedConfig { processes: Vec::new(), jobs: vec![job("unreaped", OverlapPolicy::Skip)] },
        }),
    ).await;
    let mut restarted_events = restarted_facade.subscribe_events();
    let bootstrap_facade = restarted_facade.clone();
    let mut bootstrap = tokio::spawn(async move { bootstrap_facade.bootstrap().await });
    loop {
        tokio::select! {
            result = &mut bootstrap => {
                result.unwrap().unwrap();
                break;
            }
            event = restarted_events.recv() => {
                let event = event.unwrap();
                if matches!(&event.event, DomainEvent::JobRunCancelled { run_id: event_run_id, .. } if event_run_id == &run_id) {
                    cancelled_events += 1;
                }
                if let Some(event_id) = event.event_id {
                    assert_eq!(event_id, first_event_id);
                    event.complete_delivery();
                }
            }
        }
    }
    assert_eq!(
        restarted_facade.get_run("unreaped", &run_id).await.unwrap().state,
        my_supervisor_core::domain::JobRunState::Cancelled
    );
    assert_eq!(cancelled_events, 2);
    assert!(store.pending_transient_cleanup(10).await.unwrap().is_empty());
    assert!(store.pending_transient_terminal_events(10).await.unwrap().is_empty());
}

#[tokio::test]
async fn terminal_outbox_stays_durable_when_no_external_transport_receives_it() {
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let lifecycle = Arc::new(FakeLifecycle::new(Duration::from_secs(2)));
    lifecycle.leave_unreaped_on_cancel();
    let facade = facade_with_store(lifecycle.clone(), store.clone()).await;
    facade.add_job(job("no-terminal-receiver", OverlapPolicy::Skip)).await.unwrap();
    let run_id = facade.trigger_job("no-terminal-receiver").await.unwrap();
    for _ in 0..20 {
        if lifecycle.transient_started.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    facade.cancel_run("no-terminal-receiver", run_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(facade.shutdown_all().await.is_err());
    assert_eq!(
        facade.get_run("no-terminal-receiver", &run_id).await.unwrap().state,
        my_supervisor_core::domain::JobRunState::Cancelled
    );
    assert_eq!(store.pending_transient_cleanup(10).await.unwrap().len(), 1);
    assert_eq!(store.pending_transient_terminal_events(10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn bootstrap_cancellation_retries_terminal_outbox_commit_after_file_reopen() {
    let directory = std::env::temp_dir().join(format!("my-supervisor-bootstrap-terminal-outbox-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("state.db");
    let persisted_job = job("bootstrap-terminal-outbox", OverlapPolicy::Skip);
    let pending_run_id = JobRunId::new();
    let running_run_id = JobRunId::new();
    let store = Arc::new(SqliteStore::connect(&database).await.unwrap());
    store.save_job(&persisted_job).await.unwrap();
    for (run_id, state, started_at) in [
        (pending_run_id, JobRunState::Pending, None),
        (running_run_id, JobRunState::Running, Some(Utc::now())),
    ] {
        store.save_run(&JobRun {
            run_id,
            job_name: persisted_job.name.clone(),
            job_id: persisted_job.id,
            triggered_by: TriggeredBy::Manual,
            scheduled_at: Utc::now(),
            started_at,
            ended_at: None,
            exit_code: None,
            state,
        }).await.unwrap();
    }
    store.fail_next_terminal_run_commits(1);
    let interrupted = facade_with_store_and_config(
        Arc::new(FakeLifecycle::new(Duration::ZERO)),
        store.clone(),
        Arc::new(FixedConfig {
            loaded: LoadedConfig { processes: Vec::new(), jobs: vec![persisted_job.clone()] },
        }),
    ).await;
    assert!(interrupted.bootstrap().await.is_err());
    assert!(store.pending_transient_terminal_events(10).await.unwrap().is_empty());
    assert!(matches!(
        store.get_run(&persisted_job.name, &pending_run_id).await.unwrap().unwrap().state,
        JobRunState::Pending | JobRunState::Running,
    ));
    drop(interrupted);
    drop(store);

    let reopened = Arc::new(SqliteStore::connect(&database).await.unwrap());
    let restarted = facade_with_store_and_config(
        Arc::new(FakeLifecycle::new(Duration::ZERO)),
        reopened.clone(),
        Arc::new(FixedConfig {
            loaded: LoadedConfig { processes: Vec::new(), jobs: vec![persisted_job.clone()] },
        }),
    ).await;
    restarted.bootstrap().await.unwrap();
    for run_id in [pending_run_id, running_run_id] {
        let run = reopened.get_run(&persisted_job.name, &run_id).await.unwrap().unwrap();
        assert_eq!(run.state, JobRunState::Cancelled);
        assert!(run.ended_at.is_some());
    }
    let terminal_events = reopened.pending_transient_terminal_events(10).await.unwrap();
    assert_eq!(terminal_events.len(), 2);
    assert!(terminal_events.iter().all(|event| event.state == JobRunState::Cancelled));
    assert_eq!(
        terminal_events.iter().map(|event| event.event_id).collect::<std::collections::HashSet<_>>().len(),
        2,
    );
    drop(restarted);
    drop(reopened);

    let reopened_again = SqliteStore::connect(&database).await.unwrap();
    assert_eq!(reopened_again.pending_transient_terminal_events(10).await.unwrap(), terminal_events);
    drop(reopened_again);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn bootstrap_resumes_durable_cleanup_after_daemon_restart() {
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let first_lifecycle = Arc::new(FakeLifecycle::new(Duration::from_secs(2)));
    first_lifecycle.leave_unreaped_on_cancel();
    let first_facade = facade_with_store(first_lifecycle.clone(), store.clone()).await;
    first_facade.add_job(job("restart-cleanup", OverlapPolicy::Skip)).await.unwrap();
    let run_id = first_facade.trigger_job("restart-cleanup").await.unwrap();
    for _ in 0..20 {
        if first_lifecycle.transient_started.load(Ordering::SeqCst) == 1 { break; }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    first_facade.cancel_run("restart-cleanup", run_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(store.pending_transient_cleanup(10).await.unwrap().len(), 1);

    let restarted_facade = facade_with_store_and_config(
        Arc::new(FakeLifecycle::new(Duration::ZERO)),
        store.clone(),
        Arc::new(FixedConfig {
            loaded: LoadedConfig { processes: Vec::new(), jobs: vec![job("restart-cleanup", OverlapPolicy::Skip)] },
        }),
    ).await;
    let mut events = restarted_facade.subscribe_events();
    let bootstrap_facade = restarted_facade.clone();
    let mut bootstrap = tokio::spawn(async move { bootstrap_facade.bootstrap().await });
    loop {
        tokio::select! {
            result = &mut bootstrap => {
                result.unwrap().unwrap();
                break;
            }
            event = events.recv() => {
                let event = event.unwrap();
                if event.event_id.is_some() {
                    event.complete_delivery();
                }
            }
        }
    }

    assert_eq!(
        restarted_facade.get_run("restart-cleanup", &run_id).await.unwrap().state,
        my_supervisor_core::domain::JobRunState::Cancelled
    );
    assert_eq!(restarted_facade.get_run("restart-cleanup", &run_id).await.unwrap().exit_code, Some(0));
    assert!(store.pending_transient_cleanup(10).await.unwrap().is_empty());
}

#[tokio::test]
async fn cleanup_handoff_enqueue_failure_keeps_the_active_owner_until_storage_recovers() {
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    store.fail_next_transient_cleanup_enqueues(1);
    let lifecycle = Arc::new(FakeLifecycle::new(Duration::from_secs(2)));
    lifecycle.leave_unreaped_on_cancel();
    let facade = facade_with_store(lifecycle.clone(), store.clone()).await;
    facade.add_job(job("handoff-owner", OverlapPolicy::Queue)).await.unwrap();
    let first_run = facade.trigger_job("handoff-owner").await.unwrap();
    let _queued_run = facade.trigger_job("handoff-owner").await.unwrap();
    for _ in 0..20 {
        if lifecycle.transient_started.load(Ordering::SeqCst) == 1 { break; }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    facade.cancel_run("handoff-owner", first_run).await.unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    // The injected enqueue failure leaves the first runner and overlap slot
    // owned; no durable ticket or queued replacement is allowed yet.
    assert_eq!(lifecycle.transient_started.load(Ordering::SeqCst), 1);
    assert!(store.pending_transient_cleanup(10).await.unwrap().is_empty());
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(store.pending_transient_cleanup(10).await.unwrap().len(), 1);
    assert_eq!(lifecycle.transient_started.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn unreaped_cleanup_releases_the_overlap_slot_and_force_delete_drains_retry() {
    let (facade, lifecycle) = facade(Duration::from_millis(40)).await;
    let mut events = facade.subscribe_events();
    lifecycle.leave_unreaped_on_cancel();
    facade.add_job(job("unreaped-queue", OverlapPolicy::Queue)).await.unwrap();
    let first = facade.trigger_job("unreaped-queue").await.unwrap();
    let second = facade.trigger_job("unreaped-queue").await.unwrap();
    for _ in 0..20 {
        if lifecycle.transient_started.load(Ordering::SeqCst) == 1 { break; }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    facade.cancel_run("unreaped-queue", first).await.unwrap();
    for _ in 0..20 {
        if lifecycle.transient_started.load(Ordering::SeqCst) == 2 { break; }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(lifecycle.transient_started.load(Ordering::SeqCst), 2);
    let delete_facade = facade.clone();
    let mut deletion = tokio::spawn(async move { delete_facade.delete_job("unreaped-queue", true).await });
    loop {
        tokio::select! {
            result = &mut deletion => {
                result.unwrap().unwrap();
                break;
            }
            event = events.recv() => {
                let event = event.unwrap();
                if event.event_id.is_some() {
                    event.complete_delivery();
                }
            }
        }
    }
    assert!(facade.get_job("unreaped-queue").await.is_err());
    assert!(facade.get_run("unreaped-queue", &second).await.is_err());
}

#[tokio::test]
async fn stopping_during_backoff_cancels_restart() {
    let (facade, lifecycle) = facade(Duration::ZERO).await;
    let mut spec = ProcessSpec::new("worker", "fake");
    spec.autostart = true;
    spec.lifecycle = LifecycleMode::Tied;
    spec.restart.backoff_initial = Duration::from_millis(150);
    spec.restart.backoff_max = Duration::from_millis(150);
    spec.restart.jitter = false;
    facade.add_process(spec).await.unwrap();

    let supervisor = tokio::spawn(facade.clone().run_process_supervisor_loop());
    lifecycle.mark_all_dead();
    tokio::time::sleep(Duration::from_millis(30)).await;
    facade.stop_process("worker", false).await.unwrap();
    tokio::time::sleep(Duration::from_millis(220)).await;

    assert_eq!(lifecycle.spawn_count.load(Ordering::SeqCst), 1);
    supervisor.abort();
}

#[tokio::test]
async fn dependency_success_runs_downstream() {
    let (facade, _) = facade(Duration::from_millis(20)).await;
    facade
        .add_job(job("upstream", OverlapPolicy::Skip))
        .await
        .unwrap();
    let mut downstream = job("downstream", OverlapPolicy::Skip);
    downstream.trigger = JobTrigger::DependsOn(vec!["upstream".to_string()]);
    facade.add_job(downstream).await.unwrap();

    let scheduler_loop = tokio::spawn(facade.clone().run_scheduler_loop());
    facade.trigger_job("upstream").await.unwrap();
    tokio::time::sleep(Duration::from_millis(120)).await;

    let runs = facade.list_runs("downstream", 1).await.unwrap();
    assert_eq!(runs.len(), 1);
    assert!(matches!(
        runs[0].triggered_by,
        TriggeredBy::Dependency { .. }
    ));
    scheduler_loop.abort();
}

#[tokio::test]
async fn completed_runs_are_pruned_to_the_configured_count() {
    let (facade, _) = facade(Duration::from_millis(10)).await;
    let mut retained_job = job("retained", OverlapPolicy::Queue);
    retained_job.log_retention.max_runs = Some(2);
    facade.add_job(retained_job).await.unwrap();
    for _ in 0..3 {
        facade.trigger_job("retained").await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(facade.list_runs("retained", 10).await.unwrap().len(), 2);
}

#[tokio::test]
async fn system_registered_status_reports_pid_and_resource_usage() {
    let registrar = Arc::new(FakeRegistrar::default());
    let (facade, _) = facade_with_registrar(Duration::ZERO, registrar).await;
    let mut spec = ProcessSpec::new("system-service", "fake");
    spec.management_mode = ManagementMode::SystemRegistered {
        unit_name: "com.example.system-service".to_string(),
    };
    spec.autostart = true;
    let status = facade.add_process(spec).await.unwrap();
    assert_eq!(status.state, ProcessState::Running);
    assert_eq!(status.pid, Some(4242));
    assert_eq!(status.cpu_percent, 12.5);
    assert_eq!(status.memory_bytes, 8 * 1024 * 1024);
}

#[tokio::test]
async fn detached_log_subscription_uses_the_file_follower_boundary() {
    let (facade, lifecycle) = facade(Duration::ZERO).await;
    let mut spec = ProcessSpec::new("detached", "fake");
    spec.lifecycle = LifecycleMode::Detached;
    facade.add_process(spec).await.unwrap();
    let _receiver = facade.subscribe_process_logs("detached").await.unwrap();
    assert_eq!(
        lifecycle
            .detached_subscription_count
            .load(Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn zero_log_retention_bounds_are_rejected() {
    let (facade, _) = facade(Duration::ZERO).await;
    let mut invalid_job = job("invalid-retention", OverlapPolicy::Skip);
    invalid_job.log_retention.max_runs = Some(0);
    assert!(facade.add_job(invalid_job).await.is_err());
}

#[tokio::test]
async fn deleting_a_job_removes_its_run_log_file() {
    let unique = JobRunId::new();
    let log_dir = std::env::temp_dir().join(format!("my-supervisor-job-delete-{}", unique.0));
    tokio::fs::create_dir_all(&log_dir).await.unwrap();
    let log_sink = Arc::new(InMemoryLogSink::with_log_dir(log_dir.clone()));
    let (facade, _) = facade_with_log_sink(log_sink.clone()).await;
    facade
        .add_job(job("delete-with-logs", OverlapPolicy::Skip))
        .await
        .unwrap();
    let run_id = facade.trigger_job("delete-with-logs").await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let _ = log_sink
        .append_run(run_id, LogLine::now(LogStream::Stdout, "stored"))
        .await;
    let log_path = log_dir.join(format!("run-{}.jsonl", run_id.0));
    assert!(log_path.exists());
    facade.delete_job("delete-with-logs", true).await.unwrap();
    assert!(!log_path.exists());
    tokio::fs::remove_dir(log_dir).await.unwrap();
}

#[tokio::test]
async fn bootstrap_finishes_an_atomically_committed_job_deletion_after_restart() {
    let lifecycle = Arc::new(FakeLifecycle::new(Duration::ZERO));
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let deleting_job = job("restart-delete", OverlapPolicy::Skip);
    store.save_job(&deleting_job).await.unwrap();
    let run_id = JobRunId::new();
    let journal = JobDeletionJournal {
        deletion_id: Uuid::new_v4(), job: deleting_job.clone(),
        stage: JobDeletionStage::RunsDraining, run_ids: Vec::new(), last_error: None,
    };
    store.create_job_deletion_journal(&journal).await.unwrap();
    store.save_run(&JobRun {
        run_id, job_name: deleting_job.name.clone(), job_id: deleting_job.id,
        triggered_by: TriggeredBy::Manual, scheduled_at: Utc::now(), started_at: Some(Utc::now()),
        ended_at: Some(Utc::now()), exit_code: Some(0), state: JobRunState::Succeeded,
    }).await.unwrap();
    let deleted_runs = store.commit_job_deletion_rows(journal.deletion_id, &deleting_job.name).await.unwrap();
    assert!(store.get_job_deletion_journal(&deleting_job.name).await.unwrap().is_some());
    let restarted = facade_with_store(lifecycle, store.clone()).await;
    restarted.bootstrap().await.unwrap();
    assert!(store.get_job_deletion_journal(&deleting_job.name).await.unwrap().is_none());
    assert!(store.get_job(&deleting_job.name).await.unwrap().is_none());
    assert_eq!(deleted_runs, vec![run_id]);
}

#[tokio::test]
async fn restart_retries_a_failed_atomic_job_row_deletion_without_losing_runs() {
    let lifecycle = Arc::new(FakeLifecycle::new(Duration::ZERO));
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let deleting_job = job("retry-row-delete", OverlapPolicy::Skip);
    let run_id = JobRunId::new();
    store.save_job(&deleting_job).await.unwrap();
    store.save_run(&JobRun {
        run_id,
        job_name: deleting_job.name.clone(),
        job_id: deleting_job.id,
        triggered_by: TriggeredBy::Manual,
        scheduled_at: Utc::now(),
        started_at: Some(Utc::now()),
        ended_at: Some(Utc::now()),
        exit_code: Some(0),
        state: JobRunState::Succeeded,
    }).await.unwrap();
    let journal = JobDeletionJournal {
        deletion_id: Uuid::new_v4(),
        job: deleting_job.clone(),
        stage: JobDeletionStage::RunsDraining,
        run_ids: Vec::new(),
        last_error: None,
    };
    store.create_job_deletion_journal(&journal).await.unwrap();
    store.fail_next_job_deletion_row_commits(1);

    let interrupted = facade_with_store(lifecycle.clone(), store.clone()).await;
    assert!(interrupted.delete_job(&deleting_job.name, true).await.is_err());
    assert!(store.get_job(&deleting_job.name).await.unwrap().is_some());
    assert!(store.get_run(&deleting_job.name, &run_id).await.unwrap().is_some());
    assert_eq!(
        store.get_job_deletion_journal(&deleting_job.name).await.unwrap().unwrap().stage,
        JobDeletionStage::RunsDraining,
    );

    let restarted = facade_with_store(lifecycle, store.clone()).await;
    restarted.bootstrap().await.unwrap();
    assert!(store.get_job(&deleting_job.name).await.unwrap().is_none());
    assert!(store.get_run(&deleting_job.name, &run_id).await.unwrap().is_none());
    assert!(store.pending_run_log_cleanup(10).await.unwrap().is_empty());
    assert!(store.get_job_deletion_journal(&deleting_job.name).await.unwrap().is_none());
}

#[tokio::test]
async fn failed_queued_cancellation_rolls_back_without_cancelling_any_run() {
    let lifecycle = Arc::new(FakeLifecycle::new(Duration::from_millis(100)));
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let facade = facade_with_store(lifecycle.clone(), store.clone()).await;
    facade.add_job(job("rollback-queued", OverlapPolicy::Queue)).await.unwrap();
    facade.trigger_job("rollback-queued").await.unwrap();
    for _ in 0..20 {
        if lifecycle.transient_started.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let queued_run_id = facade.trigger_job("rollback-queued").await.unwrap();
    store.fail_next_job_deletion_cancellations(1);

    assert!(facade.delete_job("rollback-queued", true).await.is_err());
    assert!(store.get_job_deletion_journal("rollback-queued").await.unwrap().is_none());
    assert_eq!(
        facade.get_run("rollback-queued", &queued_run_id).await.unwrap().state,
        JobRunState::Pending,
    );
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(lifecycle.transient_started.load(Ordering::SeqCst), 2);
    assert_eq!(
        facade.get_run("rollback-queued", &queued_run_id).await.unwrap().state,
        JobRunState::Succeeded,
    );
}

#[tokio::test]
async fn restart_only_completes_rollback_required_deletion_recovery() {
    let lifecycle = Arc::new(FakeLifecycle::new(Duration::ZERO));
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let preserved_job = job("restart-rollback", OverlapPolicy::Queue);
    let queued_run_id = JobRunId::new();
    store.save_job(&preserved_job).await.unwrap();
    store.save_run(&JobRun {
        run_id: queued_run_id,
        job_name: preserved_job.name.clone(),
        job_id: preserved_job.id,
        triggered_by: TriggeredBy::Manual,
        scheduled_at: Utc::now(),
        started_at: None,
        ended_at: None,
        exit_code: None,
        state: JobRunState::Pending,
    }).await.unwrap();
    let journal = JobDeletionJournal {
        deletion_id: Uuid::new_v4(),
        job: preserved_job.clone(),
        stage: JobDeletionStage::RollbackRequired,
        run_ids: Vec::new(),
        last_error: Some("injected journal-clear interruption".into()),
    };
    store.create_job_deletion_journal(&journal).await.unwrap();
    store.fail_next_job_deletion_journal_clears(1);

    let interrupted = facade_with_store_and_config(
        lifecycle.clone(),
        store.clone(),
        Arc::new(FixedConfig {
            loaded: LoadedConfig { processes: Vec::new(), jobs: vec![preserved_job.clone()] },
        }),
    ).await;
    assert!(interrupted.bootstrap().await.is_ok());
    assert_eq!(
        store.get_job_deletion_journal(&preserved_job.name).await.unwrap().unwrap().stage,
        JobDeletionStage::RollbackRequired,
    );
    assert_eq!(
        store.get_run(&preserved_job.name, &queued_run_id).await.unwrap().unwrap().state,
        JobRunState::Pending,
    );

    let restarted = facade_with_store_and_config(
        lifecycle,
        store.clone(),
        Arc::new(FixedConfig {
            loaded: LoadedConfig { processes: Vec::new(), jobs: vec![preserved_job.clone()] },
        }),
    ).await;
    restarted.bootstrap().await.unwrap();
    assert!(store.get_job_deletion_journal(&preserved_job.name).await.unwrap().is_none());
    assert!(store.get_job(&preserved_job.name).await.unwrap().is_some());
    assert_eq!(
        store.get_run(&preserved_job.name, &queued_run_id).await.unwrap().unwrap().state,
        JobRunState::Pending,
    );
}

#[tokio::test]
async fn rollback_direction_write_failure_reopens_as_rollback_only() {
    let directory = std::env::temp_dir().join(format!("my-supervisor-rollback-direction-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("state.db");
    let preserved_job = job("rollback-direction-reopen", OverlapPolicy::Queue);
    let queued_run_id = JobRunId::new();
    let journal = JobDeletionJournal {
        deletion_id: Uuid::new_v4(),
        job: preserved_job.clone(),
        stage: JobDeletionStage::SchedulerUnregistered,
        run_ids: Vec::new(),
        last_error: None,
    };
    let store = Arc::new(SqliteStore::connect(&database).await.unwrap());
    store.save_job(&preserved_job).await.unwrap();
    store.save_run(&JobRun {
        run_id: queued_run_id,
        job_name: preserved_job.name.clone(),
        job_id: preserved_job.id,
        triggered_by: TriggeredBy::Manual,
        scheduled_at: Utc::now(),
        started_at: None,
        ended_at: None,
        exit_code: None,
        state: JobRunState::Pending,
    }).await.unwrap();
    store.create_job_deletion_journal(&journal).await.unwrap();
    store.fail_next_job_deletion_cancellations(1);
    store.fail_next_job_deletion_rollback_direction_updates(1);
    store.fail_next_job_deletion_journal_clears(1);

    let interrupted = facade_with_store_and_config(
        Arc::new(FakeLifecycle::new(Duration::ZERO)),
        store.clone(),
        Arc::new(FixedConfig {
            loaded: LoadedConfig { processes: Vec::new(), jobs: vec![preserved_job.clone()] },
        }),
    ).await;
    assert!(interrupted.delete_job(&preserved_job.name, true).await.is_err());
    assert_eq!(
        store.get_job_deletion_journal(&preserved_job.name).await.unwrap().unwrap().stage,
        JobDeletionStage::SchedulerUnregistered,
    );
    assert_eq!(
        store.get_run(&preserved_job.name, &queued_run_id).await.unwrap().unwrap().state,
        JobRunState::Pending,
    );

    drop(interrupted);
    drop(store);

    let reopened = Arc::new(SqliteStore::connect(&database).await.unwrap());
    let restarted = facade_with_store_and_config(
        Arc::new(FakeLifecycle::new(Duration::ZERO)),
        reopened.clone(),
        Arc::new(FixedConfig {
            loaded: LoadedConfig { processes: Vec::new(), jobs: vec![preserved_job.clone()] },
        }),
    ).await;
    restarted.bootstrap().await.unwrap();

    assert!(reopened.get_job_deletion_journal(&preserved_job.name).await.unwrap().is_none());
    assert!(reopened.get_job(&preserved_job.name).await.unwrap().is_some());
    assert_eq!(
        reopened.get_run(&preserved_job.name, &queued_run_id).await.unwrap().unwrap().state,
        JobRunState::Pending,
    );
    assert!(restarted.trigger_job(&preserved_job.name).await.is_ok());

    drop(restarted);
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
}
