import type { DaemonStatus, JobRun, JobStatus, LogLine, ProcessStatus } from "./types";

export const processes: ProcessStatus[] = [
  {
    name: "api-server",
    state: "running",
    managementMode: { type: "direct" },
    pid: 12345,
    restartCount: 0,
    startedAt: "2026-04-24T00:40:12Z",
    cpuPercent: 1.8,
    memoryBytes: 48234000,
    uptime: "3h 42m",
  },
  {
    name: "worker-queue",
    state: "starting",
    managementMode: { type: "system_registered", unitName: "my-supervisor-managed-worker-queue" },
    pid: null,
    restartCount: 2,
    startedAt: "2026-04-24T03:55:01Z",
    cpuPercent: 0.7,
    memoryBytes: 33112000,
    uptime: "18s",
  },
  {
    name: "web-preview",
    state: "stopped",
    managementMode: { type: "direct" },
    pid: null,
    restartCount: 1,
    startedAt: null,
    cpuPercent: 0,
    memoryBytes: 0,
    uptime: "-",
  },
  {
    name: "backup-agent",
    state: "crashed",
    managementMode: { type: "system_registered", unitName: "my-supervisor-managed-backup-agent" },
    pid: null,
    restartCount: 5,
    startedAt: "2026-04-24T01:10:44Z",
    cpuPercent: 0,
    memoryBytes: 0,
    uptime: "-",
  },
];

export const jobs: JobStatus[] = [
  {
    name: "nightly-backup",
    trigger: { type: "cron", expr: "0 2 * * *" },
    onOverlap: "skip",
    lastRun: { runId: "01HYBACKUP", state: "succeeded", endedAt: "2026-04-24T02:00:31Z", durationSec: 31 },
    nextRunAt: "2026-04-25T02:00:00Z",
    successRateRecent: 0.96,
    dependencies: { upstream: [], downstream: ["post-backup-verify"] },
  },
  {
    name: "post-backup-verify",
    trigger: { type: "depends_on", jobs: ["nightly-backup"] },
    onOverlap: "queue",
    lastRun: { runId: "01HYVERIFY", state: "succeeded", endedAt: "2026-04-24T02:01:02Z", durationSec: 28 },
    successRateRecent: 0.91,
    dependencies: { upstream: ["nightly-backup"], downstream: [] },
  },
  {
    name: "cache-warmup",
    trigger: { type: "interval", everySec: 900 },
    onOverlap: "parallel",
    lastRun: { runId: "01HYCACHE", state: "failed", endedAt: "2026-04-24T03:50:11Z", durationSec: 7 },
    nextRunAt: "2026-04-24T04:05:00Z",
    successRateRecent: 0.78,
    dependencies: { upstream: [], downstream: [] },
  },
];

export const jobRuns: JobRun[] = [
  {
    runId: "01HYBACKUP",
    jobName: "nightly-backup",
    triggeredBy: "schedule",
    scheduledAt: "2026-04-24T02:00:00Z",
    startedAt: "2026-04-24T02:00:00Z",
    endedAt: "2026-04-24T02:00:31Z",
    exitCode: 0,
    state: "succeeded",
  },
  {
    runId: "01HYVERIFY",
    jobName: "post-backup-verify",
    triggeredBy: "dependency",
    scheduledAt: "2026-04-24T02:00:31Z",
    startedAt: "2026-04-24T02:00:34Z",
    endedAt: "2026-04-24T02:01:02Z",
    exitCode: 0,
    state: "succeeded",
  },
  {
    runId: "01HYCACHE",
    jobName: "cache-warmup",
    triggeredBy: "manual",
    scheduledAt: "2026-04-24T03:50:04Z",
    startedAt: "2026-04-24T03:50:04Z",
    endedAt: "2026-04-24T03:50:11Z",
    exitCode: 1,
    state: "failed",
  },
];

export const logs: LogLine[] = [
  {
    id: "log-1",
    timestamp: "04:01:43.211",
    source: "api-server",
    stream: "stdout",
    line: "Server listening on 127.0.0.1:3000",
  },
  {
    id: "log-2",
    timestamp: "04:01:44.018",
    source: "api-server",
    stream: "stdout",
    line: "GET /health 200 3ms",
  },
  {
    id: "log-3",
    timestamp: "04:01:45.503",
    source: "backup-agent",
    stream: "stderr",
    line: "spawn_failed: binary not found at /usr/local/bin/backup-agent",
  },
  {
    id: "log-4",
    timestamp: "04:01:47.119",
    source: "cache-warmup",
    stream: "system",
    line: "log.dropped: 42 lines dropped because the WebSocket rate limit was exceeded",
  },
  {
    id: "log-5",
    timestamp: "04:01:50.831",
    source: "worker-queue",
    stream: "stdout",
    line: "Process state changed: starting -> running",
  },
];

export const daemonStatus: DaemonStatus = {
  version: "0.1.0",
  startedAt: "2026-04-24T00:30:00Z",
  pid: 9876,
  processCount: processes.length,
  configPath: "/Users/buyonglee/.config/my-supervisor/config.toml",
  logDir: "/Users/buyonglee/Library/Logs/my-supervisor",
};
