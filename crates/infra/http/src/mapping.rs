//! Domain ↔ wire DTO mapping. The single home for the contract translation so
//! HTTP routes (and the Tauri invoke adapter in child 03) stay thin. The
//! DTO→domain direction mirrors `config::convert` by design (no shared module
//! without a dependency cycle); both compile against `shared`'s one definition.

use std::path::PathBuf;
use std::time::Duration;

use my_supervisor_application::views::{
    DaemonInfo, JobPreview, JobView, LogPage, RecoveryDiagnostics,
};
use my_supervisor_core::domain::{
    AdmissionPolicy, ApplyMode, CheckKind, CheckPolicy, ConfigApplyResult, DependencyFailurePolicy,
    GuardRestartCause, GuardState, Job, JobId, JobRun, JobRunState, JobTrigger, LifecycleMode,
    LoadedConfig, LogLine, LogRetention, LogStream, ManagementMode, MemoryPolicy, MisfirePolicy,
    OverlapPolicy, ProcessDefinitionId, ProcessOperation, ProcessOperationInstanceState,
    ProcessSpec, ProcessState, ProcessStatus, QueueOverflowPolicy, RestartPolicy, RetryPolicy,
    RollingPolicy, ShutdownPolicy, ShutdownSignal, TriggeredBy, WatchPolicy,
};
use my_supervisor_core::domain::{
    AlertEpisode, AlertRule, AlertSeverity, AlertState, DeliveryAttempt, MetricSample,
    ObservabilityPage, OperatorEvent,
};
use my_supervisor_shared::api::{
    AdmissionPolicyDto, CheckKindDto, CheckPolicyDto, ConfigApplyModeDto, ConfigApplyResultDto,
    ConfigDiffDto, DaemonStatusDto, GuardRestartCauseDto, GuardStateDto, GuardStatusDto,
    JobConfigDto, JobDependenciesDto, JobPreviewDto, JobPreviewOccurrenceDto, JobRunDto,
    JobRunStateDto, JobRunSummaryDto, JobStatusDto, JobTriggerDto, LifecycleModeDto, LogLineDto,
    LogStreamDto, LogsResponseDto, ManagementModeDto, MemoryPolicyDto, MisfirePolicyDto,
    OnDependencyFailureDto, OnOverlapDto, ProcessConfigDto, ProcessInstanceStatusDto,
    ProcessOperationDto, ProcessOperationInstanceOutcomeDto, ProcessOperationInstanceStateDto,
    ProcessStateDto, ProcessStatusDto, QueueOverflowDto, RecoveryDiagnosticDto,
    RecoveryDiagnosticsDto, RestartPolicyDto, RetryPolicyDto, RollingPolicyDto, ShutdownPolicyDto,
    ShutdownSignalDto, TriggeredByDto, WatchPolicyDto,
};
use my_supervisor_shared::api::{
    AlertEpisodeDto, AlertRuleDto, AlertSeverityDto, AlertStateDto, DeliveryAttemptDto,
    MetricSampleDto, ObservabilityPageDto, OperatorEventDto,
};

fn alert_severity_to_dto(value: AlertSeverity) -> AlertSeverityDto {
    match value {
        AlertSeverity::Info => AlertSeverityDto::Info,
        AlertSeverity::Warning => AlertSeverityDto::Warning,
        AlertSeverity::Critical => AlertSeverityDto::Critical,
    }
}
fn alert_state_to_dto(value: AlertState) -> AlertStateDto {
    match value {
        AlertState::Active => AlertStateDto::Active,
        AlertState::AcknowledgedActive => AlertStateDto::AcknowledgedActive,
        AlertState::Resolved => AlertStateDto::Resolved,
    }
}
pub fn alert_rule_to_dto(value: AlertRule) -> AlertRuleDto {
    AlertRuleDto {
        id: value.id,
        name: value.name,
        condition: value.condition,
        severity: alert_severity_to_dto(value.severity),
        cooldown_seconds: value.cooldown_seconds,
        enabled: value.enabled,
        created_at: value.created_at,
        updated_at: value.updated_at,
    }
}
pub fn operator_event_to_dto(value: OperatorEvent) -> OperatorEventDto {
    OperatorEventDto {
        id: value.id,
        occurred_at: value.occurred_at,
        source: value.source,
        kind: value.kind,
        severity: alert_severity_to_dto(value.severity),
        message: value.message,
    }
}
pub fn metric_sample_to_dto(value: MetricSample) -> MetricSampleDto {
    MetricSampleDto {
        id: value.id,
        occurred_at: value.occurred_at,
        source: value.source,
        cpu_percent: value.cpu_percent,
        memory_bytes: value.memory_bytes,
        partial_bucket: value.partial_bucket,
    }
}
pub fn alert_episode_to_dto(value: AlertEpisode) -> AlertEpisodeDto {
    AlertEpisodeDto {
        id: value.id,
        rule_id: value.rule_id,
        source: value.source,
        cause: value.cause,
        state: alert_state_to_dto(value.state),
        severity: alert_severity_to_dto(value.severity),
        opened_at: value.opened_at,
        resolved_at: value.resolved_at,
        acknowledged_at: value.acknowledged_at,
    }
}
pub fn delivery_attempt_to_dto(value: DeliveryAttempt) -> DeliveryAttemptDto {
    DeliveryAttemptDto {
        id: value.id,
        alert_id: value.alert_id,
        occurred_at: value.occurred_at,
        kind: value.kind,
        outcome: value.outcome,
        detail: value.detail,
        lease_until: value.lease_until,
    }
}
pub fn observability_page_to_dto<T, U>(
    page: ObservabilityPage<T>,
    map: impl Fn(T) -> U,
) -> ObservabilityPageDto<U> {
    ObservabilityPageDto {
        records: page.records.into_iter().map(map).collect(),
        next_cursor: page.next_cursor,
        high_watermark: page.high_watermark,
        earliest_retained_at: page.earliest_retained_at,
    }
}
use my_supervisor_shared::config::FileConfig;

// --- domain -> DTO ----------------------------------------------------------

pub fn process_state_to_dto(state: ProcessState) -> ProcessStateDto {
    match state {
        ProcessState::Starting => ProcessStateDto::Starting,
        ProcessState::Running => ProcessStateDto::Running,
        ProcessState::Stopping => ProcessStateDto::Stopping,
        ProcessState::Crashed => ProcessStateDto::Crashed,
        ProcessState::Stopped => ProcessStateDto::Stopped,
    }
}

pub fn management_mode_to_dto(mode: &ManagementMode) -> ManagementModeDto {
    match mode {
        ManagementMode::Direct => ManagementModeDto::Direct,
        ManagementMode::SystemRegistered { unit_name } => ManagementModeDto::SystemRegistered {
            unit_name: unit_name.clone(),
        },
    }
}

pub fn process_status_to_dto(status: ProcessStatus) -> ProcessStatusDto {
    ProcessStatusDto {
        definition_id: Some(status.definition_id.0),
        name: status.name,
        state: process_state_to_dto(status.state),
        management_mode: management_mode_to_dto(&status.management_mode),
        desired_instances: status.desired_instances,
        instances: status
            .instances
            .into_iter()
            .map(|instance| ProcessInstanceStatusDto {
                instance_id: instance.instance_id.0,
                ordinal: instance.ordinal,
                generation: instance.generation,
                state: process_state_to_dto(instance.state),
                pid: instance.pid,
                restart_count: instance.restart_count,
                started_at: instance.started_at,
                cpu_percent: instance.cpu_percent,
                memory_bytes: instance.memory_bytes,
            })
            .collect(),
        pid: status.pid,
        unit_name: status.unit_name,
        restart_count: status.restart_count,
        started_at: status.started_at,
        cpu_percent: status.cpu_percent,
        memory_bytes: status.memory_bytes,
        guard: status.guard.map(guard_status_to_dto),
    }
}

pub fn process_operation_to_dto(operation: ProcessOperation) -> ProcessOperationDto {
    let kind = match operation.kind {
        my_supervisor_core::domain::ProcessOperationKind::Scale => "scale",
        my_supervisor_core::domain::ProcessOperationKind::RollingRestart => "rolling_restart",
    }
    .to_string();
    ProcessOperationDto {
        operation_id: operation.operation_id,
        name: operation.name,
        kind,
        target_instances: operation.target_instances,
        phase: operation.phase,
        batch: operation.batch,
        deadline: operation.deadline,
        compensation: operation.compensation,
        completed: operation.completed,
        outcomes: operation
            .outcomes
            .into_iter()
            .map(|outcome| ProcessOperationInstanceOutcomeDto {
                instance_id: outcome.instance_id.0,
                ordinal: outcome.ordinal,
                state: match outcome.state {
                    ProcessOperationInstanceState::Completed => {
                        ProcessOperationInstanceStateDto::Completed
                    }
                    ProcessOperationInstanceState::Failed => {
                        ProcessOperationInstanceStateDto::Failed
                    }
                    ProcessOperationInstanceState::NotAttempted => {
                        ProcessOperationInstanceStateDto::NotAttempted
                    }
                    ProcessOperationInstanceState::Superseded => {
                        ProcessOperationInstanceStateDto::Superseded
                    }
                },
                failed_stage: outcome.failed_stage,
                retryable: outcome.retryable,
            })
            .collect(),
    }
}

fn guard_state_to_dto(state: GuardState) -> GuardStateDto {
    match state {
        GuardState::Unknown => GuardStateDto::Unknown,
        GuardState::Healthy => GuardStateDto::Healthy,
        GuardState::Unhealthy => GuardStateDto::Unhealthy,
        GuardState::Unsupported => GuardStateDto::Unsupported,
    }
}

fn guard_restart_cause_to_dto(cause: GuardRestartCause) -> GuardRestartCauseDto {
    match cause {
        GuardRestartCause::WatchChanged => GuardRestartCauseDto::WatchChanged,
        GuardRestartCause::MemoryCeiling => GuardRestartCauseDto::MemoryCeiling,
        GuardRestartCause::LivenessFailure => GuardRestartCauseDto::LivenessFailure,
    }
}

pub fn guard_status_to_dto(status: my_supervisor_core::domain::GuardStatus) -> GuardStatusDto {
    let snapshot = status.snapshot;
    GuardStatusDto {
        process_id: snapshot.process_id,
        native_generation: snapshot.native_generation,
        observed_at: snapshot.observed_at,
        liveness: guard_state_to_dto(snapshot.liveness),
        readiness: guard_state_to_dto(snapshot.readiness),
        memory: guard_state_to_dto(snapshot.memory),
        watch: guard_state_to_dto(snapshot.watch),
        last_restart_cause: snapshot.last_restart_cause.map(guard_restart_cause_to_dto),
        last_error: snapshot.last_error,
        is_historical: status.is_historical,
    }
}

pub fn job_trigger_to_dto(trigger: &JobTrigger) -> JobTriggerDto {
    match trigger {
        JobTrigger::Cron(expr) => JobTriggerDto::Cron { expr: expr.clone() },
        JobTrigger::Interval(d) => JobTriggerDto::Interval {
            every_sec: d.as_secs(),
        },
        JobTrigger::OneShot(at) => JobTriggerDto::OneShot { at: *at },
        JobTrigger::DependsOn(jobs) => JobTriggerDto::DependsOn { jobs: jobs.clone() },
    }
}

pub fn overlap_to_dto(p: OverlapPolicy) -> OnOverlapDto {
    match p {
        OverlapPolicy::Skip => OnOverlapDto::Skip,
        OverlapPolicy::Queue => OnOverlapDto::Queue,
        OverlapPolicy::Parallel => OnOverlapDto::Parallel,
    }
}

pub fn run_state_to_dto(s: JobRunState) -> JobRunStateDto {
    match s {
        JobRunState::Pending => JobRunStateDto::Pending,
        JobRunState::Running => JobRunStateDto::Running,
        JobRunState::Succeeded => JobRunStateDto::Succeeded,
        JobRunState::Failed => JobRunStateDto::Failed,
        JobRunState::TimedOut => JobRunStateDto::TimedOut,
        JobRunState::Cancelled => JobRunStateDto::Cancelled,
        JobRunState::Skipped => JobRunStateDto::Skipped,
    }
}

pub fn triggered_by_to_dto(t: &TriggeredBy) -> TriggeredByDto {
    match t {
        TriggeredBy::Schedule => TriggeredByDto::Schedule,
        TriggeredBy::Scheduled { .. } => TriggeredByDto::Schedule,
        TriggeredBy::Manual => TriggeredByDto::Manual,
        TriggeredBy::Dependency { upstream_run_id } => TriggeredByDto::Dependency {
            upstream_run_id: upstream_run_id.0.to_string(),
        },
    }
}

pub fn job_run_to_dto(run: &JobRun) -> JobRunDto {
    JobRunDto {
        run_id: run.run_id.0.to_string(),
        job_name: run.job_name.clone(),
        triggered_by: triggered_by_to_dto(&run.triggered_by),
        scheduled_at: run.scheduled_at,
        started_at: run.started_at,
        ended_at: run.ended_at,
        exit_code: run.exit_code,
        state: run_state_to_dto(run.state),
        original_scheduled_at: run.original_scheduled_at,
        occurrence_trigger_id: run
            .occurrence
            .as_ref()
            .map(|occurrence| occurrence.trigger_id.to_string()),
        occurrence_schedule_revision: run
            .occurrence
            .as_ref()
            .map(|occurrence| occurrence.schedule_revision),
        occurrence_attempt: run.occurrence.as_ref().map(|occurrence| occurrence.attempt),
    }
}

pub fn job_preview_to_dto(preview: JobPreview) -> JobPreviewDto {
    JobPreviewDto {
        occurrences: preview
            .occurrences
            .into_iter()
            .map(|occurrence| JobPreviewOccurrenceDto {
                scheduled_at: occurrence.scheduled_at,
                local_time: occurrence.local_time,
                timezone: occurrence.timezone,
            })
            .collect(),
    }
}

fn run_summary_to_dto(run: &JobRun) -> JobRunSummaryDto {
    let duration_sec = match (run.started_at, run.ended_at) {
        (Some(s), Some(e)) => Some((e - s).num_seconds()),
        _ => None,
    };
    JobRunSummaryDto {
        run_id: run.run_id.0.to_string(),
        state: run_state_to_dto(run.state),
        ended_at: run.ended_at,
        duration_sec,
    }
}

pub fn job_view_to_dto(view: JobView) -> JobStatusDto {
    JobStatusDto {
        name: view.job.name.clone(),
        trigger: job_trigger_to_dto(&view.job.trigger),
        on_overlap: overlap_to_dto(view.job.on_overlap),
        last_run: view.last_run.as_ref().map(run_summary_to_dto),
        next_run_at: view.next_run_at,
        success_rate_recent: view.success_rate_recent,
        dependencies: JobDependenciesDto {
            upstream: view.upstream,
            downstream: view.downstream,
        },
        timezone: Some(view.job.timezone.clone()),
        schedule_revision: view.job.schedule_revision,
        trigger_id: Some(view.job.trigger_id.to_string()),
        misfire_policy: Some(match view.job.misfire_policy {
            MisfirePolicy::Skip => MisfirePolicyDto::Skip,
            MisfirePolicy::RunOnce => MisfirePolicyDto::RunOnce,
            MisfirePolicy::CatchUp { .. } => MisfirePolicyDto::CatchUp,
        }),
        retry_policy: Some(RetryPolicyDto {
            max_attempts: view.job.retry_policy.max_attempts,
            initial_backoff_sec: view.job.retry_policy.initial_backoff.as_secs(),
            max_backoff_sec: view.job.retry_policy.max_backoff.as_secs(),
            multiplier: view.job.retry_policy.multiplier,
            jitter_percent: view.job.retry_policy.jitter_percent,
        }),
        admission: Some(AdmissionPolicyDto {
            max_concurrency: view.job.admission.max_concurrency,
            max_queue: view.job.admission.max_queue,
            overflow: match view.job.admission.overflow {
                QueueOverflowPolicy::RejectNew => QueueOverflowDto::RejectNew,
                QueueOverflowPolicy::Skip => QueueOverflowDto::Skip,
            },
        }),
    }
}

pub fn log_stream_to_dto(s: LogStream) -> LogStreamDto {
    match s {
        LogStream::Stdout => LogStreamDto::Stdout,
        LogStream::Stderr => LogStreamDto::Stderr,
        LogStream::System => LogStreamDto::System,
    }
}

pub fn log_line_to_dto(line: &LogLine) -> LogLineDto {
    LogLineDto {
        sequence: line.sequence,
        timestamp: line.timestamp,
        stream: log_stream_to_dto(line.stream),
        line: line.line.clone(),
    }
}

pub fn log_page_to_dto(page: LogPage) -> LogsResponseDto {
    LogsResponseDto {
        lines: page.lines.iter().map(log_line_to_dto).collect(),
        truncated: page.truncated,
        dropped_count: page.dropped_count,
        high_watermark: page.high_watermark,
        next_sequence: page.next_sequence,
        earliest_retained_sequence: page.earliest_retained_sequence,
        cursor_expired: page.cursor_expired,
    }
}

pub fn config_apply_mode_to_domain(mode: ConfigApplyModeDto) -> ApplyMode {
    match mode {
        ConfigApplyModeDto::Merge => ApplyMode::Merge,
        ConfigApplyModeDto::Replace => ApplyMode::Replace,
    }
}

pub fn config_apply_result_to_dto(result: ConfigApplyResult) -> ConfigApplyResultDto {
    let diff = result.diff;
    ConfigApplyResultDto {
        apply_id: result.apply_id.map(|value| value.to_string()),
        mode: match result.mode {
            ApplyMode::Merge => ConfigApplyModeDto::Merge,
            ApplyMode::Replace => ConfigApplyModeDto::Replace,
        },
        diff: ConfigDiffDto {
            added_processes: diff.added_processes,
            updated_processes: diff.updated_processes,
            removed_processes: diff.removed_processes,
            added_jobs: diff.added_jobs,
            updated_jobs: diff.updated_jobs,
            removed_jobs: diff.removed_jobs,
        },
        dry_run: result.dry_run,
    }
}

pub fn file_config_to_loaded(config: FileConfig) -> LoadedConfig {
    LoadedConfig {
        processes: config
            .processes
            .into_iter()
            .map(process_config_to_spec)
            .collect(),
        jobs: config.jobs.into_iter().map(job_config_to_job).collect(),
    }
}

pub fn daemon_info_to_dto(info: DaemonInfo) -> DaemonStatusDto {
    DaemonStatusDto {
        version: info.version,
        started_at: info.started_at,
        pid: info.pid,
        process_count: info.process_count,
        config_path: info.config_path,
        log_dir: info.log_dir,
    }
}

pub fn recovery_diagnostics_to_dto(diagnostics: RecoveryDiagnostics) -> RecoveryDiagnosticsDto {
    RecoveryDiagnosticsDto {
        records: diagnostics
            .records
            .into_iter()
            .map(|record| RecoveryDiagnosticDto {
                kind: record.kind,
                id: record.id,
                resource: record.resource,
                stage: record.stage,
                attempts: record.attempts,
                last_error: record.last_error,
            })
            .collect(),
    }
}

// --- DTO -> domain (request bodies) ----------------------------------------

fn restart_policy(dto: Option<RestartPolicyDto>) -> RestartPolicy {
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

pub fn process_config_to_spec(dto: ProcessConfigDto) -> ProcessSpec {
    let definition_id = dto
        .definition_id
        .map(ProcessDefinitionId)
        .unwrap_or_else(|| ProcessDefinitionId::from_legacy_name(&dto.name));
    let management_mode = match dto.management_mode {
        Some(ManagementModeDto::SystemRegistered { unit_name }) => {
            ManagementMode::SystemRegistered { unit_name }
        }
        _ => ManagementMode::Direct,
    };
    let lifecycle = match dto.lifecycle {
        Some(LifecycleModeDto::Detached) => LifecycleMode::Detached,
        _ => LifecycleMode::Tied,
    };
    ProcessSpec {
        definition_id,
        name: dto.name,
        command: dto.command,
        args: dto.args,
        cwd: dto.cwd.map(PathBuf::from),
        env: dto.env,
        management_mode,
        lifecycle,
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

pub fn job_trigger_to_domain(dto: JobTriggerDto) -> JobTrigger {
    match dto {
        JobTriggerDto::Cron { expr } => JobTrigger::Cron(expr),
        JobTriggerDto::Interval { every_sec } => {
            JobTrigger::Interval(Duration::from_secs(every_sec))
        }
        JobTriggerDto::OneShot { at } => JobTrigger::OneShot(at),
        JobTriggerDto::DependsOn { jobs } => JobTrigger::DependsOn(jobs),
    }
}

pub fn job_config_to_job(dto: JobConfigDto) -> Job {
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
        trigger: job_trigger_to_domain(dto.trigger),
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
            Some(MisfirePolicyDto::Skip) => MisfirePolicy::Skip,
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
