//! Map `shared` wire/config DTOs onto `core` domain types.
//!
//! NOTE: `infra/http` carries the same DTO→domain direction for request bodies.
//! The two cannot share a module without a dependency cycle (both are leaf
//! adapters over `core`+`shared`), so this small mapping is intentionally
//! mirrored. Both are compile-checked against the single DTO definitions in
//! `shared`, keeping drift low-risk.

use std::path::PathBuf;
use std::time::Duration;

use my_supervisor_core::domain::{
    DependencyFailurePolicy, Job, JobId, JobTrigger, LifecycleMode, LogRetention, ManagementMode,
    OverlapPolicy, ProcessSpec, RestartPolicy, ShutdownPolicy,
};
use my_supervisor_shared::api::{
    JobConfigDto, JobTriggerDto, LifecycleModeDto, ManagementModeDto, OnDependencyFailureDto,
    OnOverlapDto, ProcessConfigDto,
};

pub fn management_mode(dto: Option<ManagementModeDto>) -> ManagementMode {
    match dto {
        Some(ManagementModeDto::SystemRegistered { unit_name }) => {
            ManagementMode::SystemRegistered { unit_name }
        }
        _ => ManagementMode::Direct,
    }
}

pub fn lifecycle_mode(dto: Option<LifecycleModeDto>) -> LifecycleMode {
    match dto {
        Some(LifecycleModeDto::Detached) => LifecycleMode::Detached,
        _ => LifecycleMode::Tied,
    }
}

pub fn process_spec(dto: ProcessConfigDto) -> ProcessSpec {
    ProcessSpec {
        name: dto.name,
        command: dto.command,
        args: dto.args,
        cwd: dto.cwd.map(PathBuf::from),
        env: dto.env,
        management_mode: management_mode(dto.management_mode),
        lifecycle: lifecycle_mode(dto.lifecycle),
        autostart: dto.autostart.unwrap_or(false),
        restart: RestartPolicy::default(),
        shutdown: ShutdownPolicy::default(),
    }
}

pub fn job_trigger(dto: JobTriggerDto) -> JobTrigger {
    match dto {
        JobTriggerDto::Cron { expr } => JobTrigger::Cron(expr),
        JobTriggerDto::Interval { every_sec } => {
            JobTrigger::Interval(Duration::from_secs(every_sec))
        }
        JobTriggerDto::OneShot { at } => JobTrigger::OneShot(at),
        JobTriggerDto::DependsOn { jobs } => JobTrigger::DependsOn(jobs),
    }
}

pub fn job(dto: JobConfigDto) -> Job {
    Job {
        id: JobId::new(),
        name: dto.name,
        command: dto.command,
        args: dto.args,
        cwd: dto.cwd.map(PathBuf::from),
        env: dto.env,
        trigger: job_trigger(dto.trigger),
        on_overlap: match dto.on_overlap {
            Some(OnOverlapDto::Queue) => OverlapPolicy::Queue,
            Some(OnOverlapDto::Parallel) => OverlapPolicy::Parallel,
            _ => OverlapPolicy::Skip,
        },
        on_dependency_failure: match dto.on_dependency_failure {
            Some(OnDependencyFailureDto::RunAnyway) => DependencyFailurePolicy::RunAnyway,
            _ => DependencyFailurePolicy::Skip,
        },
        timeout: dto.timeout_sec.map(Duration::from_secs),
        log_retention: LogRetention::default(),
    }
}
