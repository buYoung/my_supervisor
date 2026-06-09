/**
 * Tauri `invoke` adapter for the OperationsClient interface (production path inside Tauri).
 * Calls the exact command names the Tauri host exposes (briefs ...-03-tauri-bridge.md); each
 * returns the SAME snake_case DTO as the matching HTTP endpoint. All wire shapes are mapped
 * through the shared wire-mapping layer; this adapter holds no domain logic.
 *
 * @tauri-apps/api is imported dynamically so the standalone (non-Tauri) build does not
 * hard-fail when the package is absent — this adapter is only selected inside Tauri.
 */

import {
  type FollowLogsHandlers,
  type JobRunsResult,
  OperationsError,
  type OperationsClient,
  type ProcessLogsTail,
} from "./operations-client";
import {
  mapDaemonStatus,
  mapJobRun,
  mapJobStatus,
  mapLogLine,
  mapProcessStatus,
} from "./wire-mapping";
import type {
  DaemonStatusDto,
  JobConfigDto,
  JobStatusDto,
  ListJobsDto,
  ListProcessesDto,
  ListRunsDto,
  LogLineDto,
  ProcessConfigDto,
  ProcessLogsDto,
  ProcessStatusDto,
  TriggerJobDto,
} from "./wire-types";

type InvokeFn = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
type ListenFn = (
  event: string,
  handler: (event: { payload: unknown }) => void,
) => Promise<() => void>;

/** Lazily load Tauri's invoke. Wrapped so a missing/failed import surfaces as OperationsError. */
async function loadInvoke(): Promise<InvokeFn> {
  try {
    const core = await import("@tauri-apps/api/core");
    return core.invoke as InvokeFn;
  } catch (cause) {
    throw new OperationsError("transport_error", "Tauri invoke 모듈을 불러올 수 없습니다.", cause);
  }
}

async function loadListen(): Promise<ListenFn> {
  try {
    const event = await import("@tauri-apps/api/event");
    return event.listen as unknown as ListenFn;
  } catch (cause) {
    throw new OperationsError("transport_error", "Tauri event 모듈을 불러올 수 없습니다.", cause);
  }
}

/** Normalize a thrown invoke rejection (often the daemon's error envelope) to OperationsError. */
function toOperationsError(cause: unknown): OperationsError {
  if (cause instanceof OperationsError) {
    return cause;
  }
  if (cause && typeof cause === "object" && "error" in cause) {
    const envelope = (cause as { error?: { code?: string; message?: string; details?: unknown } })
      .error;
    if (envelope) {
      return new OperationsError(
        envelope.code ?? "internal_error",
        envelope.message ?? "데몬 오류가 발생했습니다.",
        envelope.details,
      );
    }
  }
  const message =
    typeof cause === "string"
      ? cause
      : cause instanceof Error
        ? cause.message
        : "데몬 호출에 실패했습니다.";
  return new OperationsError("internal_error", message, cause);
}

async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const invoke = await loadInvoke();
  try {
    return await invoke<T>(command, args);
  } catch (cause) {
    throw toOperationsError(cause);
  }
}

export function createInvokeClient(): OperationsClient {
  return {
    transport: "invoke",

    async listProcesses() {
      const dto = await invokeCommand<ListProcessesDto>("cmd_list_processes");
      return dto.processes.map(mapProcessStatus);
    },

    async getProcess(name) {
      const dto = await invokeCommand<ProcessStatusDto>("cmd_get_process", { name });
      return mapProcessStatus(dto);
    },

    async addProcess(config: ProcessConfigDto) {
      const dto = await invokeCommand<ProcessStatusDto>("cmd_add_process", { config });
      return mapProcessStatus(dto);
    },

    async startProcess(name) {
      await invokeCommand<void>("cmd_start_process", { name });
    },

    async stopProcess(name, force = false) {
      await invokeCommand<void>("cmd_stop_process", { name, force });
    },

    async restartProcess(name) {
      await invokeCommand<void>("cmd_restart_process", { name });
    },

    async removeProcess(name, force = false) {
      await invokeCommand<void>("cmd_remove_process", { name, force });
    },

    async convertProcess(name, to, options = {}) {
      // Tauri v2 maps camelCase JS args to snake_case Rust params (unit_name, auto_start).
      const dto = await invokeCommand<ProcessStatusDto>("cmd_convert_process", {
        name,
        to,
        unitName: options.unitName,
        autoStart: options.autoStart,
      });
      return mapProcessStatus(dto);
    },

    async processLogsTail(name, tail = 100) {
      const dto = await invokeCommand<ProcessLogsDto>("cmd_process_logs", { name, tail });
      const lines = dto.lines.map((line, index) => mapLogLine(line, name, index));
      const result: ProcessLogsTail = {
        lines,
        truncated: dto.truncated,
        droppedCount: dto.dropped_count,
      };
      return result;
    },

    followProcessLogs(name, handlers: FollowLogsHandlers) {
      let counter = 0;
      let unlisten: (() => void) | null = null;
      let cancelled = false;

      // Listen on the per-process Tauri event channel `process-log:{name}`, THEN
      // ask the host to start forwarding the broadcast to that channel. Order
      // matters: register the listener first so no early lines are missed.
      void loadListen()
        .then((listen) => listen(`process-log:${name}`, (event) => {
          const payload = event.payload as LogLineDto;
          handlers.onLine(mapLogLine(payload, name, counter));
          counter += 1;
        }))
        .then(async (dispose) => {
          if (cancelled) {
            dispose();
            return;
          }
          unlisten = dispose;
          // Without this the Rust side never subscribes/emits and the stream is silent.
          await invokeCommand<void>("cmd_follow_logs", { name });
        })
        .catch((cause) => {
          handlers.onError?.(toOperationsError(cause));
        });

      return () => {
        cancelled = true;
        unlisten?.();
      };
    },

    async listJobs() {
      const dto = await invokeCommand<ListJobsDto>("cmd_list_jobs");
      return dto.jobs.map(mapJobStatus);
    },

    async addJob(config: JobConfigDto) {
      const dto = await invokeCommand<JobStatusDto>("cmd_add_job", { config });
      return mapJobStatus(dto);
    },

    async removeJob(name, force = false) {
      await invokeCommand<void>("cmd_remove_job", { name, force });
    },

    async triggerJob(name) {
      const dto = await invokeCommand<TriggerJobDto>("cmd_trigger_job", { name });
      return { runId: dto.run_id };
    },

    async listRuns(name, limit = 50) {
      const dto = await invokeCommand<ListRunsDto>("cmd_list_runs", { name, limit });
      const result: JobRunsResult = {
        runs: dto.runs.map(mapJobRun),
        truncated: dto.truncated,
      };
      return result;
    },

    async daemonStatus() {
      const dto = await invokeCommand<DaemonStatusDto>("cmd_daemon_status");
      return mapDaemonStatus(dto);
    },
  };
}
