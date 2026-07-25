/**
 * Transport-agnostic operations client interface. The three feature views consume THIS
 * interface only — never a concrete transport. Two adapters implement it:
 *
 *  - createInvokeClient()  — Tauri `invoke` (production path inside the desktop app).
 *  - createHttpClient()    — HTTP fetch + WebSocket (standalone, against the daemon).
 *
 * Selection happens in services/index.ts via runtime Tauri detection. Both adapters use the
 * shared wire-mapping layer, so the camelCase shapes the views see are identical regardless
 * of transport.
 */

import type {
  DaemonStatus,
  JobRun,
  JobStatus,
  LogLine,
  ProcessStatus,
} from "../shared/types";
import type { ConvertTargetDto, JobConfigDto, ProcessConfigDto } from "./wire-types";

const EVENT_DEDUP_CACHE_CAPACITY = 1_024;

/** Target management mode for a convert request. */
export type ConvertTarget = ConvertTargetDto;

/** Normalized error envelope (docs/API.md §5). `code` is "transport_error" when the request never reached the daemon. */
export class OperationsError extends Error {
  readonly code: string;
  readonly details?: unknown;

  constructor(code: string, message: string, details?: unknown) {
    super(message);
    this.name = "OperationsError";
    this.code = code;
    this.details = details;
  }
}

/** Seed result for the Logs view: the recent tail before the live follow takes over. */
export interface ProcessLogsTail {
  lines: LogLine[];
  truncated: boolean;
  droppedCount: number;
}

/** Result of merging per-job run histories for the cross-job 실행 이력 table. */
export interface JobRunsResult {
  runs: JobRun[];
  truncated: boolean;
}

export interface FollowLogsHandlers {
  /** Called for each live log line. */
  onLine: (line: LogLine) => void;
  /** Called when the server inserts a rate-limit drop control frame (DD-012). */
  onDropped?: (count: number) => void;
  /** Called when the underlying stream errors or closes unexpectedly. */
  onError?: (error: OperationsError) => void;
}

/** Transport-independent global event shape exposed to desktop features. */
export interface EventEnvelope {
  eventType: string;
  eventId?: string;
  timestamp: string;
  payload: unknown;
}

export interface FollowEventsHandlers {
  /** Called once per accepted event. Durable terminal duplicates share eventId. */
  onEvent: (event: EventEnvelope) => void;
  /** Called for an unexpected transport failure before a reconnect attempt. */
  onError?: (error: OperationsError) => void;
}

/**
 * Return session-memory ID de-duplication for terminal event consumers. Server
 * durability belongs to the SQLite outbox; this bounded cache intentionally
 * disappears when the renderer reloads and accepts ID-less legacy envelopes.
 */
export function createEventDeduper(capacity = EVENT_DEDUP_CACHE_CAPACITY): (eventId?: string) => boolean {
  const eventIds = new Set<string>();
  const insertionOrder: string[] = [];
  return (eventId?: string): boolean => {
    if (!eventId) {
      return true;
    }
    if (eventIds.has(eventId)) {
      return false;
    }
    eventIds.add(eventId);
    insertionOrder.push(eventId);
    if (insertionOrder.length > capacity) {
      const expiredEventId = insertionOrder.shift();
      if (expiredEventId) {
        eventIds.delete(expiredEventId);
      }
    }
    return true;
  };
}

export interface OperationsClient {
  /** Identifies which transport is active (for diagnostics / handoff visibility). */
  readonly transport: "invoke" | "http";

  // Processes
  listProcesses(): Promise<ProcessStatus[]>;
  getProcess(name: string): Promise<ProcessStatus>;
  addProcess(config: ProcessConfigDto): Promise<ProcessStatus>;
  startProcess(name: string): Promise<void>;
  stopProcess(name: string, force?: boolean): Promise<void>;
  restartProcess(name: string): Promise<void>;
  removeProcess(name: string, force?: boolean): Promise<void>;
  /**
   * Convert a process between Direct and SystemRegistered (launchd) modes
   * (docs/API.md §2, child 06). `unitName` defaults server-side when omitted.
   */
  convertProcess(
    name: string,
    to: ConvertTarget,
    options?: { unitName?: string; autoStart?: boolean },
  ): Promise<ProcessStatus>;
  processLogsTail(name: string, tail?: number): Promise<ProcessLogsTail>;
  /** Follow a single process's logs. Returns an unsubscribe function. */
  followProcessLogs(name: string, handlers: FollowLogsHandlers): () => void;
  /** Follow global events; terminal duplicates are removed in renderer session memory. */
  followEvents(handlers: FollowEventsHandlers): () => void;

  // Jobs
  listJobs(): Promise<JobStatus[]>;
  addJob(config: JobConfigDto): Promise<JobStatus>;
  removeJob(name: string, force?: boolean): Promise<void>;
  triggerJob(name: string): Promise<{ runId: string }>;
  listRuns(name: string, limit?: number): Promise<JobRunsResult>;

  // Daemon
  daemonStatus(): Promise<DaemonStatus>;
}
