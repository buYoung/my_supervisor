//! `ProcessJobRunner` — the `JobRunner` port implemented by composing
//! `LifecycleController` controlled transient execution + `JobRepository` (DD: application
//! composition, no new crate).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::broadcast;

use my_supervisor_core::domain::{
    Job, JobRun, JobRunId, JobRunState, LifecycleMode, ManagementMode, ProcessSpec, RestartPolicy,
    ShutdownPolicy, TriggeredBy,
};
use my_supervisor_core::ports::error::RunnerError;
use my_supervisor_core::ports::{
    JobRepository, JobRunner, LifecycleController, RunExecutionControl, SystemClock,
    CleanupTicket, TransientCompletion, TransientTerminalEvent, LogSink,
};

use crate::events::{DomainEvent, PublishedEvent};

/// Build a transient Direct/detached `ProcessSpec` from a Job definition.
fn job_to_spec(job: &Job) -> ProcessSpec {
    ProcessSpec {
        name: format!("job:{}", job.name),
        command: job.command.clone(),
        args: job.args.clone(),
        cwd: job.cwd.clone(),
        env: job.env.clone(),
        management_mode: ManagementMode::Direct,
        lifecycle: LifecycleMode::Detached,
        autostart: false,
        restart: RestartPolicy {
            enabled: false,
            ..RestartPolicy::default()
        },
        shutdown: ShutdownPolicy::default(),
    }
}

pub struct ProcessJobRunner {
    lifecycle: Arc<dyn LifecycleController>,
    job_repo: Arc<dyn JobRepository>,
    log_sink: Arc<dyn LogSink>,
    clock: Arc<dyn SystemClock>,
    events: broadcast::Sender<PublishedEvent>,
    internal_events: broadcast::Sender<PublishedEvent>,
}

impl ProcessJobRunner {
    pub fn new(
        lifecycle: Arc<dyn LifecycleController>,
        job_repo: Arc<dyn JobRepository>,
        log_sink: Arc<dyn LogSink>,
        clock: Arc<dyn SystemClock>,
        events: broadcast::Sender<PublishedEvent>,
        internal_events: broadcast::Sender<PublishedEvent>,
    ) -> Self {
        ProcessJobRunner {
            lifecycle,
            job_repo,
            log_sink,
            clock,
            events,
            internal_events,
        }
    }

    async fn enqueue_cleanup_ticket(
        &self,
        job: &Job,
        run_id: JobRunId,
        control: &Arc<dyn RunExecutionControl>,
        stage: my_supervisor_core::ports::TransientCleanupStage,
        intended_terminal_state: JobRunState,
        cause: String,
        outcome: my_supervisor_core::ports::TransientOutcome,
    ) -> Result<(), RunnerError> {
        let child = control.child().ok_or_else(|| {
            RunnerError::Backend("transient cleanup lost its verified child handle".into())
        })?;
        let ticket = CleanupTicket {
            cleanup_id: uuid::Uuid::new_v4(),
            job_id: job.id,
            job_name: job.name.clone(),
            run_id,
            child,
            stage,
            attempts: 0,
            last_error: Some(cause),
            intended_terminal_state,
            outcome,
        };
        // A failed durable handoff must retain the active runner (and hence
        // the adapter's child ownership) until storage is available again.
        // Returning a generic backend error here used to make the facade drop
        // that only owner while the group was still alive.
        loop {
            match self.job_repo.enqueue_transient_cleanup(&ticket).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    tracing::warn!(run_id = %run_id.0, error = %error, "transient cleanup handoff is waiting for durable storage");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }
}

#[async_trait]
impl JobRunner for ProcessJobRunner {
    async fn run(
        &self,
        job: &Job,
        triggered_by: TriggeredBy,
        run_id: JobRunId,
        control: Arc<dyn RunExecutionControl>,
    ) -> Result<JobRun, RunnerError> {
        let scheduled_at = self.clock.now();

        let mut run = JobRun {
            run_id,
            job_name: job.name.clone(),
            job_id: job.id,
            triggered_by,
            scheduled_at,
            started_at: Some(scheduled_at),
            ended_at: None,
            exit_code: None,
            state: JobRunState::Running,
        };
        // The facade persisted Pending before registering this active run. This
        // transition is best-effort; only a valid, non-tombstoned final save is
        // authoritative.
        let _ = self.job_repo.save_run(&run).await;
        let _ = self.events.send(PublishedEvent::ordinary(DomainEvent::JobRunStarted {
            name: job.name.clone(),
            run_id,
        }));

        let spec = job_to_spec(job);
        let execution_result = async {
            let child = self.lifecycle.start_transient(&spec, run_id).await?;
            control.publish_child(child.clone()).await;
            let mut cancellation = control.cancellation();
            self.lifecycle
                .complete_transient(&child, job.timeout, &mut cancellation)
                .await
        }
        .await;

        match execution_result {
            Ok(TransientCompletion::Exited(outcome)) => {
                run.started_at = Some(outcome.started_at);
                run.ended_at = Some(outcome.ended_at);
                run.exit_code = outcome.exit_code;
                run.state = if outcome.exit_code == Some(0) {
                    JobRunState::Succeeded
                } else {
                    JobRunState::Failed
                };
            }
            Ok(TransientCompletion::TimedOut(outcome)) => {
                run.started_at = Some(outcome.started_at);
                run.ended_at = Some(outcome.ended_at);
                run.exit_code = outcome.exit_code;
                run.state = JobRunState::TimedOut;
            }
            Ok(TransientCompletion::Cancelled(outcome)) => {
                run.started_at = Some(outcome.started_at);
                run.ended_at = Some(outcome.ended_at);
                run.exit_code = outcome.exit_code;
                run.state = JobRunState::Cancelled;
            }
            Ok(TransientCompletion::CleanupPending { cause, stage, intended_terminal_state, outcome }) => {
                self.enqueue_cleanup_ticket(
                    job,
                    run_id,
                    &control,
                    stage,
                    intended_terminal_state,
                    cause.clone(),
                    outcome,
                ).await?;
                return Err(RunnerError::Unreaped(cause));
            }
            Err(e) => {
                tracing::warn!(job = %job.name, error = %e, "job run failed to launch");
                run.ended_at = Some(self.clock.now());
                run.state = JobRunState::Failed;
            }
        }

        // A force-deleted definition may have been recreated under the same
        // name while this child was being reaped. The registry tombstone and
        // repository's JobId condition both turn that late completion into a
        // normal closure rather than attaching it to the replacement job.
        if let Err(error) = self.log_sink.seal_run(run_id).await {
            let cause = error.to_string();
            self.enqueue_cleanup_ticket(
                job,
                run_id,
                &control,
                my_supervisor_core::ports::TransientCleanupStage::SealLog,
                run.state,
                cause.clone(),
                my_supervisor_core::ports::TransientOutcome {
                    started_at: run.started_at.unwrap_or(scheduled_at),
                    ended_at: run.ended_at.unwrap_or_else(|| self.clock.now()),
                    exit_code: run.exit_code,
                },
            ).await?;
            return Err(RunnerError::Unreaped(cause));
        }
        if !control.should_persist_terminal() {
            return Ok(run);
        }
        let terminal_event = TransientTerminalEvent {
            cleanup_id: uuid::Uuid::new_v4(),
            event_id: uuid::Uuid::new_v4(),
            occurred_at: run.ended_at.unwrap_or_else(|| self.clock.now()),
            job_name: job.name.clone(),
            run_id,
            state: run.state,
            exit_code: run.exit_code,
        };
        match self
            .job_repo
            .commit_terminal_run_with_event(&run, &terminal_event)
            .await
        {
            Ok(()) => {}
            Err(my_supervisor_core::ports::RepoError::Conflict(_)) => return Ok(run),
            Err(error) => {
                let cause = error.to_string();
                self.enqueue_cleanup_ticket(
                    job,
                    run_id,
                    &control,
                    my_supervisor_core::ports::TransientCleanupStage::PersistTerminal,
                run.state,
                cause.clone(),
                my_supervisor_core::ports::TransientOutcome {
                    started_at: run.started_at.unwrap_or(scheduled_at),
                    ended_at: run.ended_at.unwrap_or_else(|| self.clock.now()),
                    exit_code: run.exit_code,
                },
                ).await?;
                return Err(RunnerError::Unreaped(cause));
            }
        }

        let event = match run.state {
            JobRunState::Succeeded => DomainEvent::JobRunSucceeded {
                name: job.name.clone(),
                run_id,
                exit_code: run.exit_code.unwrap_or(0),
            },
            JobRunState::TimedOut => DomainEvent::JobRunTimedOut {
                name: job.name.clone(),
                run_id,
            },
            JobRunState::Cancelled => DomainEvent::JobRunCancelled {
                name: job.name.clone(),
                run_id,
            },
            _ => DomainEvent::JobRunFailed {
                name: job.name.clone(),
                run_id,
                exit_code: run.exit_code,
            },
        };
        // Scheduler dependency handling must advance once the Run row is
        // durable; external transports receive the matching stable-ID event
        // exclusively through the outbox worker.
        let _ = self.internal_events.send(PublishedEvent::ordinary(event));

        Ok(run)
    }
}
