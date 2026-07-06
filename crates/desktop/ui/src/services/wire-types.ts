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
}

export interface ListProcessesDto {
  processes: ProcessStatusDto[];
}

export interface ProcessLogsDto {
  lines: LogLineDto[];
  truncated: boolean;
  dropped_count: number;
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
}

export interface ListJobsDto {
  jobs: JobStatusDto[];
}

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

export interface DaemonStatusDto {
  version: string;
  started_at: string;
  pid: number;
  process_count: number;
  config_path: string;
  log_dir: string;
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
