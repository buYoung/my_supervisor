/**
 * Snake_case wire DTOs as served by the daemon (docs/API.md §4) and returned by the
 * matching Tauri invoke commands (docs/briefs ...-03-tauri-bridge.md). These are the
 * AUTHORITATIVE on-the-wire shapes; the camelCase shapes in shared/types.ts are the FE
 * source of truth. The wire-mapping module converts between them — neither adapter pushes
 * camelCase onto the wire.
 */

import type { JobRunState, JobTrigger, ProcessState } from "../shared/types";

export type ManagementModeDto =
  | { type: "direct" }
  | { type: "system_registered"; unit_name: string };

/** Target mode for `POST /api/v1/processes/{name}/convert` (docs/API.md §2). */
export type ConvertTargetDto = "direct" | "system_registered";

export type GuardStateDto = "unknown" | "healthy" | "unhealthy" | "unsupported";
export type GuardRestartCauseDto = "watch_changed" | "memory_ceiling" | "liveness_failure";

export interface GuardStatusDto {
  process_id: string;
  native_generation: string | null;
  observed_at: string;
  liveness: GuardStateDto;
  readiness: GuardStateDto;
  memory: GuardStateDto;
  watch: GuardStateDto;
  last_restart_cause: GuardRestartCauseDto | null;
  last_error: string | null;
  is_historical: boolean;
}

export interface ProcessStatusDto {
  name: string;
  state: ProcessState;
  management_mode: ManagementModeDto;
  pid: number | null;
  unit_name: string | null;
  restart_count: number;
  started_at: string | null;
  cpu_percent: number;
  memory_bytes: number;
  /** Absent when connected to an older daemon. */
  guard?: GuardStatusDto;
}

export interface ListProcessesDto {
  processes: ProcessStatusDto[];
}
export interface ProcessPageDto { processes: ProcessStatusDto[]; next_cursor?: string | null; high_watermark: string; partial?: boolean; failed_partitions?: string[]; }

export interface ProcessInstanceStatusDto {
  instance_id: string;
  ordinal: number;
  generation: number;
  state: ProcessState;
  pid: number | null;
  restart_count: number;
  started_at: string | null;
  cpu_percent: number;
  memory_bytes: number;
}

export interface ProcessInstancesDto { name: string; desired_instances: number; instances: ProcessInstanceStatusDto[]; }
export interface ProcessOperationDto {
  operation_id: string; name: string; kind: string; target_instances?: number; phase: string; batch: number; completed: boolean;
  outcomes: Array<{ instance_id: string; ordinal: number; state: "completed" | "failed" | "not_attempted" | "superseded"; failed_stage?: string; retryable: boolean }>;
}

export interface ProcessLogsDto {
  lines: LogLineDto[];
  truncated: boolean;
  dropped_count: number;
  /** Absent when connected to an older daemon. */
  earliest_retained_sequence?: number | null;
  /** Absent when connected to an older daemon. */
  cursor_expired?: boolean;
}

export interface JobRunSummaryDto {
  run_id: string;
  state: JobRunState;
  ended_at?: string;
  duration_sec?: number;
}

export interface JobStatusDto {
  name: string;
  trigger: JobTrigger;
  on_overlap: "skip" | "queue" | "parallel";
  last_run?: JobRunSummaryDto;
  next_run_at?: string;
  success_rate_recent?: number;
  dependencies: { upstream: string[]; downstream: string[] };
  timezone?: string;
  misfire_policy?: "skip" | "run_once" | "catch_up";
}

export interface ListJobsDto {
  jobs: JobStatusDto[];
}
export interface JobPageDto { jobs: JobStatusDto[]; next_cursor?: string | null; high_watermark: string; partial?: boolean; failed_partitions?: string[]; }

export type TriggeredByDto =
  | { type: "schedule" }
  | { type: "manual" }
  | { type: "dependency"; upstream_run_id: string };

export interface JobRunDto {
  run_id: string;
  job_name: string;
  triggered_by: TriggeredByDto;
  scheduled_at: string;
  started_at?: string;
  ended_at?: string;
  exit_code?: number;
  state: JobRunState;
}

export interface ListRunsDto {
  runs: JobRunDto[];
  truncated: boolean;
}

export interface TriggerJobDto {
  run_id: string;
}

export interface JobPreviewRequestDto { config: JobConfigDto; at: string; count?: number; }
export interface JobPreviewDto { occurrences: Array<{ scheduled_at: string; local_time: string; timezone: string }>; }

export interface DaemonStatusDto {
  version: string;
  started_at: string;
  pid: number;
  process_count: number;
  config_path: string;
  log_dir: string;
}

/** Global `/api/v1/events` and Tauri `global-event` envelope. `event_id` is
 * optional so a renderer can interoperate with an older daemon. */
export interface EventEnvelopeDto {
  type: string;
  event_id?: string;
  timestamp: string;
  payload: unknown;
}

/** A single wire log line, shared by REST `/logs` and the per-process WS stream. */
export interface LogLineDto {
  timestamp: string;
  stream: "stdout" | "stderr" | "system";
  line: string;
}

/** The WS control frame inserted when the per-process rate limit is exceeded (DD-012). */
export interface LogDroppedFrameDto {
  type: "log.dropped";
  payload: { count: number };
}

/**
 * A snake_case ProcessConfig body (docs/API.md §4.3). The add-process form supplies a
 * minimal subset; the daemon fills defaults. Kept open-ended on purpose.
 */
export interface ProcessConfigDto {
  name: string;
  command: string;
  args?: string[];
  cwd?: string;
  env?: Record<string, string>;
  management_mode?: ManagementModeDto;
  [key: string]: unknown;
}

/** A snake_case JobConfig body (docs/API.md §4.4). The add-job form supplies a subset. */
export interface JobConfigDto {
  name: string;
  command: string;
  args?: string[];
  cwd?: string;
  env?: Record<string, string>;
  trigger: JobTrigger;
  on_overlap?: "skip" | "queue" | "parallel";
  timeout_sec?: number;
  [key: string]: unknown;
}
