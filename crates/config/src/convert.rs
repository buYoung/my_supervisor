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
    OverlapPolicy, ProcessSpec, RestartPolicy, ShutdownPolicy, ShutdownSignal,
};
use my_supervisor_shared::api::{
    JobConfigDto, JobTriggerDto, LifecycleModeDto, ManagementModeDto, OnDependencyFailureDto,
    OnOverlapDto, ProcessConfigDto, RestartPolicyDto, ShutdownPolicyDto, ShutdownSignalDto,
};

pub fn management_mode(dto: Option<ManagementModeDto>) -> ManagementMode {
    match dto {
        Some(ManagementModeDto::SystemRegistered { unit_name }) => {
            ManagementMode::SystemRegistered { unit_name }
        }
        _ => ManagementMode::Direct,
    }
}

fn shutdown_policy(dto: Option<ShutdownPolicyDto>) -> ShutdownPolicy {
    let defaults = ShutdownPolicy::default();
    let Some(dto) = dto else {
        return defaults;
    };
    ShutdownPolicy {
        signal: match dto.signal {
            Some(ShutdownSignalDto::Int) => ShutdownSignal::Int,
            Some(ShutdownSignalDto::Kill) => ShutdownSignal::Kill,
            _ => ShutdownSignal::Term,
        },
        grace_period: Duration::from_millis(
            dto.grace_period_ms
                .unwrap_or(defaults.grace_period.as_millis() as u64),
        ),
    }
}

pub fn lifecycle_mode(dto: Option<LifecycleModeDto>) -> LifecycleMode {
    match dto {
        Some(LifecycleModeDto::Detached) => LifecycleMode::Detached,
        _ => LifecycleMode::Tied,
    }
}

pub fn restart_policy(dto: Option<RestartPolicyDto>) -> RestartPolicy {
    let defaults = RestartPolicy::default();
    let Some(dto) = dto else {
        return defaults;
    };
    RestartPolicy {
        enabled: dto.enabled.unwrap_or(defaults.enabled),
        max_retries: dto.max_retries,
        backoff_initial: Duration::from_millis(
            dto.backoff_initial_ms
                .unwrap_or(defaults.backoff_initial.as_millis() as u64),
        ),
        backoff_max: Duration::from_millis(
            dto.backoff_max_ms
                .unwrap_or(defaults.backoff_max.as_millis() as u64),
        ),
        backoff_multiplier: dto
            .backoff_multiplier
            .unwrap_or(defaults.backoff_multiplier),
        jitter: dto.jitter.unwrap_or(defaults.jitter),
        reset_after: Duration::from_millis(
            dto.reset_after_ms
                .unwrap_or(defaults.reset_after.as_millis() as u64),
        ),
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
        restart: restart_policy(dto.restart),
        shutdown: shutdown_policy(dto.shutdown),
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
        log_retention: dto
            .log_retention
            .map(|retention| LogRetention {
                max_runs: retention.max_runs,
                max_age_days: retention.max_age_days,
            })
            .unwrap_or_default(),
    }
}
