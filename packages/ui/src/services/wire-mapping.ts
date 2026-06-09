/**
 * Shared wire-mapping layer used by BOTH transport adapters (invoke + HTTP/WS).
 * Converts the snake_case wire DTOs (services/wire-types.ts) into the camelCase FE
 * shapes (shared/types.ts), reconciling the four divergent fields:
 *
 *  1. ProcessStatus.uptime  — derived from started_at ("2h 5m"; "-" when null).
 *  2. JobRun.triggeredBy    — flattened from the tagged triggered_by object to a string union.
 *  3. LogLine.id            — synthesized stable list key (`${timestamp}-${index}`).
 *  4. LogLine.source        — filled from the selected process name (the wire line has none).
 *
 * types.ts stays the single FE source of truth; nothing here is pushed back onto the wire.
 */

import type {
  DaemonStatus,
  JobRun,
  JobStatus,
  LogLine,
  ManagementMode,
  ProcessStatus,
} from "../shared/types";
import type {
  DaemonStatusDto,
  JobRunDto,
  JobStatusDto,
  LogLineDto,
  ManagementModeDto,
  ProcessStatusDto,
  TriggeredByDto,
} from "./wire-types";

/** Derive a compact human uptime string from an RFC3339 start timestamp. "-" when null. */
export function formatUptime(startedAt: string | null, nowMs: number = Date.now()): string {
  if (!startedAt) {
    return "-";
  }

  const startedMs = Date.parse(startedAt);
  if (Number.isNaN(startedMs)) {
    return "-";
  }

  const elapsedSeconds = Math.max(0, Math.floor((nowMs - startedMs) / 1000));

  if (elapsedSeconds < 60) {
    return `${elapsedSeconds}s`;
  }

  const totalMinutes = Math.floor(elapsedSeconds / 60);
  const minutes = totalMinutes % 60;
  const totalHours = Math.floor(totalMinutes / 60);
  const hours = totalHours % 24;
  const days = Math.floor(totalHours / 24);

  if (days > 0) {
    return `${days}d ${hours}h`;
  }
  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  return `${minutes}m`;
}

function mapManagementMode(mode: ManagementModeDto): ManagementMode {
  if (mode.type === "system_registered") {
    return { type: "system_registered", unitName: mode.unit_name };
  }
  return { type: "direct" };
}

export function mapProcessStatus(dto: ProcessStatusDto): ProcessStatus {
  return {
    name: dto.name,
    state: dto.state,
    managementMode: mapManagementMode(dto.management_mode),
    pid: dto.pid,
    restartCount: dto.restart_count,
    startedAt: dto.started_at,
    cpuPercent: dto.cpu_percent,
    memoryBytes: dto.memory_bytes,
    uptime: formatUptime(dto.started_at),
  };
}

export function mapJobStatus(dto: JobStatusDto): JobStatus {
  return {
    name: dto.name,
    trigger: dto.trigger,
    onOverlap: dto.on_overlap,
    lastRun: dto.last_run
      ? {
          runId: dto.last_run.run_id,
          state: dto.last_run.state,
          endedAt: dto.last_run.ended_at,
          durationSec: dto.last_run.duration_sec,
        }
      : undefined,
    nextRunAt: dto.next_run_at,
    successRateRecent: dto.success_rate_recent,
    dependencies: dto.dependencies,
  };
}

/** Flatten the wire's tagged triggered_by object to the flat FE string union. */
function flattenTriggeredBy(triggeredBy: TriggeredByDto): JobRun["triggeredBy"] {
  return triggeredBy.type;
}

export function mapJobRun(dto: JobRunDto): JobRun {
  return {
    runId: dto.run_id,
    jobName: dto.job_name,
    triggeredBy: flattenTriggeredBy(dto.triggered_by),
    scheduledAt: dto.scheduled_at,
    startedAt: dto.started_at,
    endedAt: dto.ended_at,
    exitCode: dto.exit_code,
    state: dto.state,
  };
}

export function mapDaemonStatus(dto: DaemonStatusDto): DaemonStatus {
  return {
    version: dto.version,
    startedAt: dto.started_at,
    pid: dto.pid,
    processCount: dto.process_count,
    configPath: dto.config_path,
    logDir: dto.log_dir,
  };
}

/**
 * Map a wire log line to a FE LogLine. `source` is supplied by the caller (the selected
 * process name, which the wire line omits) and `id` is synthesized from the timestamp plus
 * a caller-provided index/counter so list keys stay stable and unique.
 */
export function mapLogLine(dto: LogLineDto, source: string, index: number): LogLine {
  return {
    id: `${dto.timestamp}-${index}`,
    timestamp: dto.timestamp,
    source,
    stream: dto.stream,
    line: dto.line,
  };
}
