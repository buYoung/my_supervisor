export type ThemePreference = "auto" | "dark" | "light";

export type NavigationKey = "processes" | "jobs" | "logs" | "daemon" | "settings";

export type ProcessState = "starting" | "running" | "stopping" | "crashed" | "stopped";

export type ManagementMode =
  | { type: "direct" }
  | { type: "system_registered"; unitName: string };

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
}

export type JobRunState =
  | "pending"
  | "running"
  | "succeeded"
  | "failed"
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
