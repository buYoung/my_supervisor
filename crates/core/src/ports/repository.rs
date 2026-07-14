//! Persistence ports. `StateRepository` owns the process registry (specs +
//! restart counters); `JobRepository` owns job definitions and run history.

use async_trait::async_trait;

use crate::domain::{ChildHandle, Job, JobRun, JobRunId, ProcessSpec};
use crate::ports::error::RepoError;

/// Durable process registry. Survives daemon restart so the managed-process
/// list is remembered; live runtime status is held in memory by the supervisor.
#[async_trait]
pub trait StateRepository: Send + Sync {
    async fn list_specs(&self) -> Result<Vec<ProcessSpec>, RepoError>;
    async fn get_spec(&self, name: &str) -> Result<Option<ProcessSpec>, RepoError>;
    async fn save_spec(&self, spec: &ProcessSpec) -> Result<(), RepoError>;
    async fn delete_spec(&self, name: &str) -> Result<(), RepoError>;
    async fn get_restart_count(&self, name: &str) -> Result<u32, RepoError>;
    async fn set_restart_count(&self, name: &str, count: u32) -> Result<(), RepoError>;
    async fn get_runtime_handle(&self, name: &str) -> Result<Option<ChildHandle>, RepoError>;
    async fn set_runtime_handle(
        &self,
        name: &str,
        handle: Option<&ChildHandle>,
    ) -> Result<(), RepoError>;
}

/// Job definitions plus run history.
#[async_trait]
pub trait JobRepository: Send + Sync {
    async fn list_jobs(&self) -> Result<Vec<Job>, RepoError>;
    async fn get_job(&self, name: &str) -> Result<Option<Job>, RepoError>;
    async fn save_job(&self, job: &Job) -> Result<(), RepoError>;
    async fn delete_job(&self, name: &str) -> Result<(), RepoError>;
    async fn save_run(&self, run: &JobRun) -> Result<(), RepoError>;
    async fn list_runs(&self, job_name: &str, limit: usize) -> Result<Vec<JobRun>, RepoError>;
    async fn get_run(&self, job_name: &str, run_id: &JobRunId)
        -> Result<Option<JobRun>, RepoError>;
}
