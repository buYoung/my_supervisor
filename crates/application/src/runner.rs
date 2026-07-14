//! `ProcessJobRunner` — the `JobRunner` port implemented by composing
//! `LifecycleController::run_transient` + `JobRepository` (DD: application
//! composition, no new crate).

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;

use my_supervisor_core::domain::{
    Job, JobRun, JobRunId, JobRunState, LifecycleMode, ManagementMode, ProcessSpec, RestartPolicy,
    ShutdownPolicy, TriggeredBy,
};
use my_supervisor_core::ports::error::RunnerError;
use my_supervisor_core::ports::{JobRepository, JobRunner, LifecycleController, SystemClock};

use crate::events::DomainEvent;

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
    clock: Arc<dyn SystemClock>,
    events: broadcast::Sender<DomainEvent>,
}

impl ProcessJobRunner {
    pub fn new(
        lifecycle: Arc<dyn LifecycleController>,
        job_repo: Arc<dyn JobRepository>,
        clock: Arc<dyn SystemClock>,
        events: broadcast::Sender<DomainEvent>,
    ) -> Self {
        ProcessJobRunner {
            lifecycle,
            job_repo,
            clock,
            events,
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
    ) -> Result<JobRun, RunnerError> {
        let scheduled_at = self.clock.now();

        let mut run = JobRun {
            run_id,
            job_name: job.name.clone(),
            triggered_by,
            scheduled_at,
            started_at: Some(scheduled_at),
            ended_at: None,
            exit_code: None,
            state: JobRunState::Running,
        };
        // Best-effort persist of the in-flight run; the final save is authoritative.
        let _ = self.job_repo.save_run(&run).await;
        let _ = self.events.send(DomainEvent::JobRunStarted {
            name: job.name.clone(),
            run_id,
        });

        let spec = job_to_spec(job);
        let execution_result = if let Some(timeout) = job.timeout {
            match tokio::time::timeout(timeout, self.lifecycle.run_transient(&spec, run_id)).await {
                Ok(result) => Some(result),
                Err(_) => {
                    tracing::warn!(job = %job.name, ?timeout, "job run timed out");
                    None
                }
            }
        } else {
            Some(self.lifecycle.run_transient(&spec, run_id).await)
        };

        match execution_result {
            Some(Ok(outcome)) => {
                run.started_at = Some(outcome.started_at);
                run.ended_at = Some(outcome.ended_at);
                run.exit_code = outcome.exit_code;
                run.state = if outcome.exit_code == Some(0) {
                    JobRunState::Succeeded
                } else {
                    JobRunState::Failed
                };
            }
            Some(Err(e)) => {
                tracing::warn!(job = %job.name, error = %e, "job run failed to launch");
                run.ended_at = Some(self.clock.now());
                run.state = JobRunState::Failed;
            }
            None => {
                run.ended_at = Some(self.clock.now());
                run.state = JobRunState::Failed;
            }
        }

        self.job_repo
            .save_run(&run)
            .await
            .map_err(|e| RunnerError::Backend(e.to_string()))?;

        let event = match run.state {
            JobRunState::Succeeded => DomainEvent::JobRunSucceeded {
                name: job.name.clone(),
                run_id,
                exit_code: run.exit_code.unwrap_or(0),
            },
            _ => DomainEvent::JobRunFailed {
                name: job.name.clone(),
                run_id,
                exit_code: run.exit_code,
            },
        };
        let _ = self.events.send(event);

        Ok(run)
    }
}
