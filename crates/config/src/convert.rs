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
    AdmissionPolicy, CheckKind, CheckPolicy, DependencyFailurePolicy, Job, JobId, JobTrigger,
    LifecycleMode, LogRetention, ManagementMode, MemoryPolicy, MisfirePolicy, OverlapPolicy,
    ProcessDefinitionId, ProcessSpec, QueueOverflowPolicy, RestartPolicy, RetryPolicy,
    RollingPolicy, ShutdownPolicy, ShutdownSignal, WatchPolicy,
};
use my_supervisor_shared::api::{
    CheckKindDto, CheckPolicyDto, JobConfigDto, JobTriggerDto, LifecycleModeDto, ManagementModeDto,
    MemoryPolicyDto, MisfirePolicyDto, OnDependencyFailureDto, OnOverlapDto, ProcessConfigDto,
    QueueOverflowDto, RestartPolicyDto, RollingPolicyDto, ShutdownPolicyDto, ShutdownSignalDto,
    WatchPolicyDto,
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

fn watch_policy(dto: Option<WatchPolicyDto>) -> Option<WatchPolicy> {
    dto.map(|dto| WatchPolicy {
        roots: dto.roots.into_iter().map(PathBuf::from).collect(),
        recursive: dto.recursive.unwrap_or(false),
        exclusions: dto.exclusions.into_iter().map(PathBuf::from).collect(),
        follow_symlinks: dto.follow_symlinks.unwrap_or(false),
        debounce: Duration::from_millis(dto.debounce_ms),
    })
}

fn memory_policy(dto: Option<MemoryPolicyDto>) -> Option<MemoryPolicy> {
    dto.map(|dto| MemoryPolicy {
        ceiling_bytes: dto.ceiling_bytes,
        sample_interval: Duration::from_millis(dto.sample_interval_ms),
        consecutive_breaches: dto.consecutive_breaches,
    })
}

fn check_policy(dto: Option<CheckPolicyDto>) -> Option<CheckPolicy> {
    dto.map(|dto| CheckPolicy {
        kind: match dto.kind {
            CheckKindDto::Exec { command, args } => CheckKind::Exec { command, args },
            CheckKindDto::Tcp { host, port } => CheckKind::Tcp { host, port },
            CheckKindDto::Http {
                url,
                expected_status,
            } => CheckKind::Http {
                url,
                expected_status,
            },
        },
        interval: Duration::from_millis(dto.interval_ms),
        timeout: Duration::from_millis(dto.timeout_ms),
        consecutive_successes: dto.consecutive_successes,
        consecutive_failures: dto.consecutive_failures,
    })
}

fn rolling_policy(dto: Option<RollingPolicyDto>) -> Option<RollingPolicy> {
    dto.map(|dto| RollingPolicy {
        max_surge: dto.max_surge,
        max_unavailable: dto.max_unavailable,
        readiness_timeout: Duration::from_millis(dto.readiness_timeout_ms),
        routability: dto.routability.unwrap_or(false),
    })
}

pub fn process_spec(dto: ProcessConfigDto) -> ProcessSpec {
    let definition_id = dto
        .definition_id
        .map(ProcessDefinitionId)
        .unwrap_or_else(|| ProcessDefinitionId::from_legacy_name(&dto.name));
    ProcessSpec {
        definition_id,
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
        instances: dto.instances.unwrap_or(1),
        watch: watch_policy(dto.watch),
        memory: memory_policy(dto.memory),
        liveness: check_policy(dto.liveness),
        readiness: check_policy(dto.readiness),
        rolling: rolling_policy(dto.rolling),
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
    let overlap = match dto.on_overlap {
        Some(OnOverlapDto::Queue) => OverlapPolicy::Queue,
        Some(OnOverlapDto::Parallel) => OverlapPolicy::Parallel,
        _ => OverlapPolicy::Skip,
    };
    Job {
        id: JobId::new(),
        name: dto.name,
        command: dto.command,
        args: dto.args,
        cwd: dto.cwd.map(PathBuf::from),
        env: dto.env,
        trigger: job_trigger(dto.trigger),
        on_overlap: overlap,
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
        // New configuration input must get the same local-time contract as
        // HTTP/Tauri input. A sentinel deliberately reaches application
        // validation if the host cannot resolve its IANA timezone; silently
        // selecting UTC would create a different schedule than the operator
        // asked for.
        timezone: dto.timezone.unwrap_or_else(|| {
            iana_time_zone::get_timezone()
                .unwrap_or_else(|_| "__unresolved_local_timezone__".to_string())
        }),
        schedule_revision: dto.schedule_revision.unwrap_or(0),
        trigger_id: dto
            .trigger_id
            .and_then(|value| uuid::Uuid::parse_str(&value).ok())
            .unwrap_or_else(uuid::Uuid::new_v4),
        misfire_policy: match dto.misfire_policy {
            Some(MisfirePolicyDto::RunOnce) => MisfirePolicy::RunOnce,
            Some(MisfirePolicyDto::CatchUp) => MisfirePolicy::CatchUp {
                max_occurrences: 100,
                max_age: Duration::from_secs(86400),
            },
            _ => MisfirePolicy::RunOnce,
        },
        retry_policy: dto
            .retry_policy
            .map(|policy| RetryPolicy {
                max_attempts: policy.max_attempts.max(1),
                initial_backoff: Duration::from_secs(policy.initial_backoff_sec),
                max_backoff: Duration::from_secs(policy.max_backoff_sec),
                multiplier: policy.multiplier.max(1),
                jitter_percent: policy.jitter_percent.min(100),
            })
            .unwrap_or_default(),
        admission: dto
            .admission
            .map(|policy| AdmissionPolicy {
                max_concurrency: policy.max_concurrency.max(1),
                max_queue: policy.max_queue,
                overflow: match policy.overflow {
                    QueueOverflowDto::Skip => QueueOverflowPolicy::Skip,
                    QueueOverflowDto::RejectNew => QueueOverflowPolicy::RejectNew,
                },
            })
            .unwrap_or_else(|| AdmissionPolicy::legacy(overlap)),
    }
}
