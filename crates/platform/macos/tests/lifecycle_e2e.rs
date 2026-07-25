use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use my_supervisor_core::domain::{JobId, JobRunId, JobRunState, ProcessSpec, ShutdownPolicy, ShutdownSignal};
use my_supervisor_core::ports::{CleanupTicket, LifecycleController, LogSink, ShutdownSignaler, TransientCleanupStage, TransientCompletion, TransientOutcome};
use my_supervisor_infra_logging::InMemoryLogSink;
use my_supervisor_platform_macos::{MacLifecycle, UnixShutdown};

fn temporary_directory() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("my-supervisor-lifecycle-e2e-{}", uuid::Uuid::new_v4()))
}

async fn wait_for_group_exit(process_group: u32) {
    for _ in 0..80 {
        // SAFETY: signal 0 to the dedicated test process group has no side effect.
        let exists = unsafe { libc::kill(-(process_group as i32), 0) } == 0;
        if !exists {
            assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("test process group {process_group} still exists after controlled cancellation");
}

#[tokio::test]
async fn cancellation_reaps_the_leader_and_grandchild_process_group() {
    let directory = temporary_directory();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(directory.clone()));
    let lifecycle = MacLifecycle::new(sink, directory.clone());
    let mut spec = ProcessSpec::new("lifecycle-e2e", "/bin/sh");
    spec.args = vec!["-c".into(), "sleep 30 & wait".into()];

    let handle = lifecycle.start_transient(&spec, JobRunId::new()).await.unwrap();
    let process_group = handle.pgid.expect("transient child owns a dedicated process group");
    let (cancel, mut receiver) = tokio::sync::watch::channel(false);
    cancel.send(true).unwrap();
    let completion = lifecycle.complete_transient(&handle, None, &mut receiver).await.unwrap();
    assert!(matches!(completion, TransientCompletion::Cancelled(_)));
    wait_for_group_exit(process_group).await;

    tokio::fs::remove_dir_all(directory).await.unwrap();
}

#[tokio::test]
async fn immediate_term_cancellation_reaps_each_owned_child_without_unreaped_completion() {
    let directory = temporary_directory();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(directory.clone()));
    let lifecycle = MacLifecycle::new(sink, directory.clone());

    for _ in 0..8 {
        let mut spec = ProcessSpec::new("immediate-term-e2e", "/bin/sh");
        spec.args = vec!["-c".into(), "trap 'exit 0' TERM; while :; do :; done".into()];
        let handle = lifecycle.start_transient(&spec, JobRunId::new()).await.unwrap();
        let process_group = handle.pgid.expect("transient child owns a dedicated process group");
        let (_cancel, mut receiver) = tokio::sync::watch::channel(true);

        let completion = lifecycle.complete_transient(&handle, None, &mut receiver).await.unwrap();
        assert!(matches!(completion, TransientCompletion::Cancelled(_)));
        wait_for_group_exit(process_group).await;
    }

    tokio::fs::remove_dir_all(directory).await.unwrap();
}

#[tokio::test]
async fn direct_stop_reaps_a_term_ignoring_grandchild_after_its_leader_exits() {
    let directory = temporary_directory();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(directory.clone()));
    let lifecycle = MacLifecycle::new(sink, directory.clone());
    let mut spec = ProcessSpec::new("direct-whole-group-e2e", "/bin/sh");
    spec.args = vec![
        "-c".into(),
        "trap 'exit 0' TERM; (trap '' TERM; while :; do sleep 1; done) & wait".into(),
    ];

    let handle = lifecycle.spawn_tied(&spec).await.unwrap();
    let process_group = handle.pgid.expect("tied child owns a dedicated process group");
    UnixShutdown::new()
        .request_graceful(
            &handle,
            &ShutdownPolicy {
                signal: ShutdownSignal::Term,
                grace_period: Duration::from_millis(100),
            },
        )
        .await
        .unwrap();
    wait_for_group_exit(process_group).await;

    tokio::fs::remove_dir_all(directory).await.unwrap();
}

#[tokio::test]
async fn transient_handles_use_native_microsecond_generations() {
    let directory = temporary_directory();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(directory.clone()));
    let lifecycle = MacLifecycle::new(sink, directory.clone());
    let mut spec = ProcessSpec::new("generation-e2e", "/bin/sleep");
    spec.args = vec!["30".into()];

    let first = lifecycle.start_transient(&spec, JobRunId::new()).await.unwrap();
    let second = lifecycle.start_transient(&spec, JobRunId::new()).await.unwrap();
    assert!(first.generation.as_deref().is_some_and(|value| value.starts_with("macos-libproc:")));
    assert!(second.generation.as_deref().is_some_and(|value| value.starts_with("macos-libproc:")));
    assert_ne!(first.generation, second.generation);

    let (cancel, mut receiver) = tokio::sync::watch::channel(true);
    drop(cancel);
    assert!(matches!(lifecycle.complete_transient(&first, None, &mut receiver).await.unwrap(), TransientCompletion::Cancelled(_)));
    let (_cancel, mut receiver) = tokio::sync::watch::channel(true);
    assert!(matches!(lifecycle.complete_transient(&second, None, &mut receiver).await.unwrap(), TransientCompletion::Cancelled(_)));
    tokio::fs::remove_dir_all(directory).await.unwrap();
}

#[tokio::test]
async fn restart_cleanup_reaps_a_live_group_and_preserves_the_recorded_outcome() {
    let directory = temporary_directory();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(directory.clone()));
    let first_lifecycle = MacLifecycle::new(sink.clone(), directory.clone());
    let mut spec = ProcessSpec::new("restart-live-group-e2e", "/bin/sh");
    spec.args = vec!["-c".into(), "(trap '' TERM; while :; do sleep 1; done) & wait".into()];
    let handle = first_lifecycle.start_transient(&spec, JobRunId::new()).await.unwrap();
    let process_group = handle.pgid.expect("transient child owns a dedicated process group");
    let outcome = TransientOutcome {
        started_at: handle.started_at,
        ended_at: Utc::now(),
        exit_code: Some(42),
    };
    let ticket = CleanupTicket {
        cleanup_id: uuid::Uuid::new_v4(),
        job_id: JobId(uuid::Uuid::new_v4()),
        job_name: "restart-live-group-e2e".into(),
        run_id: JobRunId::new(),
        child: handle,
        stage: TransientCleanupStage::TerminateGroup,
        attempts: 0,
        last_error: Some("injected daemon stop".into()),
        intended_terminal_state: JobRunState::Cancelled,
        outcome,
    };
    drop(first_lifecycle);

    let restarted_lifecycle = MacLifecycle::new(sink, directory.clone());
    let completion = restarted_lifecycle.resume_transient_cleanup(&ticket).await.unwrap();
    assert_eq!(completion, TransientCompletion::Cancelled(outcome));
    wait_for_group_exit(process_group).await;
    tokio::fs::remove_dir_all(directory).await.unwrap();
}
