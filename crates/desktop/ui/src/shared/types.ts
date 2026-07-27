export type ThemePreference = "auto" | "dark" | "light";

export type NavigationKey = "processes" | "jobs" | "logs" | "daemon" | "settings";

export type ProcessState = "starting" | "running" | "stopping" | "crashed" | "stopped";

export type ManagementMode =
  | { type: "direct" }
  | { type: "system_registered"; unitName: string };

export type GuardState = "unknown" | "healthy" | "unhealthy" | "unsupported";
export type GuardRestartCause = "watch_changed" | "memory_ceiling" | "liveness_failure";

export interface GuardStatus {
  processId: string;
  nativeGeneration: string | null;
  observedAt: string;
  liveness: GuardState;
  readiness: GuardState;
  memory: GuardState;
  watch: GuardState;
  lastRestartCause: GuardRestartCause | null;
  lastError: string | null;
  isHistorical: boolean;
}

export interface ProcessStatus {
  name: string;
  state: ProcessState;
  managementMode: ManagementMode;
  pid: number | null;
  restartCount: number;
  startedAt: string | null;
  cpuPercent: number;
  memoryBytes: number;
  uptime: string;
  guard?: GuardStatus;
}

export interface ProcessInstanceStatus {
  instanceId: string;
  ordinal: number;
  generation: number;
  state: ProcessState;
  pid: number | null;
  restartCount: number;
  startedAt: string | null;
  cpuPercent: number;
  memoryBytes: number;
}

export interface ProcessOperationOutcome {
  instanceId: string;
  ordinal: number;
  state: "completed" | "failed" | "not_attempted" | "superseded";
  failedStage?: string;
  retryable: boolean;
}

export interface ProcessOperation {
  operationId: string;
  name: string;
  kind: string;
  targetInstances?: number;
  phase: string;
  batch: number;
  completed: boolean;
  outcomes: ProcessOperationOutcome[];
}

export type JobRunState =
  | "pending"
  | "running"
  | "succeeded"
  | "failed"
  | "timed_out"
  | "cancelled"
  | "skipped";

export type JobTrigger =
  | { type: "cron"; expr: string }
  | { type: "interval"; everySec: number }
  | { type: "one_shot"; at: string }
  | { type: "depends_on"; jobs: string[] };

export interface JobRunSummary {
  runId: string;
  state: JobRunState;
  endedAt?: string;
  durationSec?: number;
}

export interface JobStatus {
  name: string;
  trigger: JobTrigger;
  onOverlap: "skip" | "queue" | "parallel";
  lastRun?: JobRunSummary;
  nextRunAt?: string;
  successRateRecent?: number;
  dependencies: { upstream: string[]; downstream: string[] };
  timezone?: string;
  misfirePolicy?: "skip" | "run_once" | "catch_up";
}

export interface JobRun {
  runId: string;
  jobName: string;
  triggeredBy: "schedule" | "manual" | "dependency";
  scheduledAt: string;
  startedAt?: string;
  endedAt?: string;
  exitCode?: number;
  state: JobRunState;
}

export interface JobPreviewOccurrence { scheduledAt: string; localTime: string; timezone: string; }
export interface JobPreview { occurrences: JobPreviewOccurrence[]; }

export interface LogLine {
  id: string;
  timestamp: string;
  source: string;
  stream: "stdout" | "stderr" | "system";
  line: string;
}

export interface DaemonStatus {
  version: string;
  startedAt: string;
  pid: number;
  processCount: number;
  configPath: string;
  logDir: string;
}
