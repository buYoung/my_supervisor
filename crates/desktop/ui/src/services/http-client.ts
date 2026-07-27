/**
 * HTTP fetch + WebSocket adapter for the OperationsClient interface (standalone path).
 * Talks to the daemon at VITE_API_BASE_URL (default http://127.0.0.1:9876). Loopback,
 * no-auth — no token is ever sent (DD-011). All wire shapes are mapped through the shared
 * wire-mapping layer; this adapter holds no domain logic.
 */

import type { JobStatus, ProcessStatus } from "../shared/types";
import {
  createEventDeduper,
  type EventEnvelope,
  type FollowEventsHandlers,
  type FollowLogsHandlers,
  type JobRunsResult,
  OperationsError,
  type OperationsClient,
  type ProcessLogsTail,
  type ResourcePage,
} from "./operations-client";
import {
  mapDaemonStatus,
  mapEventEnvelope,
  mapJobRun,
  mapJobPreview,
  mapJobStatus,
  mapLogLine,
  mapProcessStatus,
  mapProcessInstanceStatus,
  mapProcessOperation,
} from "./wire-mapping";
import type {
  DaemonStatusDto,
  EventEnvelopeDto,
  JobConfigDto,
  JobPreviewDto,
  JobPreviewRequestDto,
  JobStatusDto,
  JobPageDto,
  ListJobsDto,
  ListProcessesDto,
  ProcessPageDto,
  ListRunsDto,
  LogDroppedFrameDto,
  LogLineDto,
  ProcessConfigDto,
  ProcessLogsDto,
  ProcessStatusDto,
  ProcessInstancesDto,
  ProcessOperationDto,
  JobRunDto,
  TriggerJobDto,
} from "./wire-types";

const DEFAULT_BASE_URL = "http://127.0.0.1:9876";

function resolveBaseUrl(): string {
  const configured = import.meta.env.VITE_API_BASE_URL;
  return (configured ?? DEFAULT_BASE_URL).replace(/\/+$/, "");
}

/** Build the ws(s):// origin from the http(s):// base URL. */
function toWebSocketOrigin(baseUrl: string): string {
  return baseUrl.replace(/^http(s?):\/\//, (_match, secure: string) => `ws${secure}://`);
}

interface ErrorEnvelope {
  error?: { code?: string; message?: string; details?: unknown };
}

function isEventEnvelopeDto(value: unknown): value is EventEnvelopeDto {
  return Boolean(
    value
      && typeof value === "object"
      && typeof (value as { type?: unknown }).type === "string"
      && typeof (value as { timestamp?: unknown }).timestamp === "string"
      && "payload" in value,
  );
}

async function parseErrorEnvelope(response: Response): Promise<OperationsError> {
  let code = "internal_error";
  let message = `HTTP ${response.status}`;
  let details: unknown;
  try {
    const body = (await response.json()) as ErrorEnvelope;
    if (body?.error) {
      code = body.error.code ?? code;
      message = body.error.message ?? message;
      details = body.error.details;
    }
  } catch {
    // Non-JSON or empty error body — keep the status-based defaults.
  }
  return new OperationsError(code, message, details);
}

export function createHttpClient(): OperationsClient {
  const baseUrl = resolveBaseUrl();
  const webSocketOrigin = toWebSocketOrigin(baseUrl);

  /** fetch wrapper: normalizes transport failures and error envelopes to OperationsError. */
  async function request(path: string, init?: RequestInit): Promise<Response> {
    let response: Response;
    try {
      response = await fetch(`${baseUrl}${path}`, {
        ...init,
        headers: { "Content-Type": "application/json", ...init?.headers },
      });
    } catch (cause) {
      throw new OperationsError(
        "transport_error",
        cause instanceof Error ? cause.message : "데몬에 연결할 수 없습니다.",
        cause,
      );
    }
    if (!response.ok) {
      throw await parseErrorEnvelope(response);
    }
    return response;
  }

  async function requestJson<T>(path: string, init?: RequestInit): Promise<T> {
    const response = await request(path, init);
    return (await response.json()) as T;
  }

  /** For 202/204 endpoints with empty or ignorable bodies — never parses the body. */
  async function requestNoContent(path: string, init?: RequestInit): Promise<void> {
    await request(path, init);
  }

  const encode = (name: string) => encodeURIComponent(name);

  return {
    transport: "http",

    async listProcesses() {
      const dto = await requestJson<ListProcessesDto>("/api/v1/processes");
      return dto.processes.map(mapProcessStatus);
    },

    async listProcessesPage(cursor, highWatermark, limit) {
      const query = new URLSearchParams();
      if (cursor) query.set("cursor", cursor);
      if (highWatermark) query.set("high_watermark", highWatermark);
      if (limit !== undefined) query.set("limit", String(limit));
      const dto = await requestJson<ProcessPageDto>(`/api/v1/processes/page?${query}`);
      const result: ResourcePage<ProcessStatus> = { records: dto.processes.map(mapProcessStatus), nextCursor: dto.next_cursor ?? undefined, highWatermark: dto.high_watermark, partial: dto.partial ?? false, failedPartitions: dto.failed_partitions ?? [] };
      return result;
    },

    async getProcess(name) {
      const dto = await requestJson<ProcessStatusDto>(`/api/v1/processes/${encode(name)}`);
      return mapProcessStatus(dto);
    },

    async processInstances(name) {
      const dto = await requestJson<ProcessInstancesDto>(`/api/v1/processes/${encode(name)}/instances`);
      return { name: dto.name, desiredInstances: dto.desired_instances, instances: dto.instances.map(mapProcessInstanceStatus) };
    },

    async scaleProcess(name, instances, operationId) {
      return mapProcessOperation(await requestJson<ProcessOperationDto>(`/api/v1/processes/${encode(name)}/scale`, { method: "POST", headers: operationId ? { "Idempotency-Key": operationId } : undefined, body: JSON.stringify({ instances, operation_id: operationId }) }));
    },

    async rollingRestartProcess(name, operationId) {
      return mapProcessOperation(await requestJson<ProcessOperationDto>(`/api/v1/processes/${encode(name)}/rolling-restart`, { method: "POST", headers: operationId ? { "Idempotency-Key": operationId } : undefined, body: JSON.stringify({ operation_id: operationId }) }));
    },

    async addProcess(config: ProcessConfigDto) {
      const dto = await requestJson<ProcessStatusDto>("/api/v1/processes", {
        method: "POST",
        body: JSON.stringify(config),
      });
      return mapProcessStatus(dto);
    },

    async startProcess(name) {
      await requestNoContent(`/api/v1/processes/${encode(name)}/start`, { method: "POST" });
    },

    async stopProcess(name, force = false) {
      const query = force ? "?force=true" : "";
      await requestNoContent(`/api/v1/processes/${encode(name)}/stop${query}`, { method: "POST" });
    },

    async restartProcess(name) {
      // 202 (Direct) or 200 {noop, reason} (SystemRegistered) — body is ignored either way.
      await requestNoContent(`/api/v1/processes/${encode(name)}/restart`, { method: "POST" });
    },

    async removeProcess(name, force = false) {
      const query = force ? "?force=true" : "";
      await requestNoContent(`/api/v1/processes/${encode(name)}${query}`, { method: "DELETE" });
    },

    async convertProcess(name, to, options = {}) {
      const dto = await requestJson<ProcessStatusDto>(
        `/api/v1/processes/${encode(name)}/convert`,
        {
          method: "POST",
          body: JSON.stringify({
            to,
            unit_name: options.unitName,
            auto_start: options.autoStart,
          }),
        },
      );
      return mapProcessStatus(dto);
    },

    async processLogsTail(name, tail = 100) {
      const dto = await requestJson<ProcessLogsDto>(
        `/api/v1/processes/${encode(name)}/logs?tail=${tail}`,
      );
      const lines = dto.lines.map((line, index) => mapLogLine(line, name, index));
      const result: ProcessLogsTail = {
        lines,
        truncated: dto.truncated,
        droppedCount: dto.dropped_count,
        earliestRetainedSequence: dto.earliest_retained_sequence ?? undefined,
        cursorExpired: dto.cursor_expired ?? false,
      };
      return result;
    },

    followProcessLogs(name, handlers: FollowLogsHandlers) {
      const socket = new WebSocket(`${webSocketOrigin}/api/v1/processes/${encode(name)}/logs`);
      let counter = 0;
      let closedByCaller = false;

      socket.addEventListener("message", (event) => {
        let parsed: LogLineDto | LogDroppedFrameDto;
        try {
          parsed = JSON.parse(event.data as string);
        } catch {
          return;
        }
        // Branch on the control-frame shape (§3.2): drop frames carry no log line.
        if ("type" in parsed && parsed.type === "log.dropped") {
          handlers.onDropped?.(parsed.payload.count);
          return;
        }
        handlers.onLine(mapLogLine(parsed as LogLineDto, name, counter));
        counter += 1;
      });

      socket.addEventListener("error", () => {
        if (!closedByCaller) {
          handlers.onError?.(
            new OperationsError("transport_error", "로그 스트림 연결에 실패했습니다."),
          );
        }
      });

      return () => {
        closedByCaller = true;
        if (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING) {
          socket.close();
        }
      };
    },

    followEvents(handlers: FollowEventsHandlers) {
      const shouldEmit = createEventDeduper();
      let socket: WebSocket | null = null;
      let retryTimer: number | undefined;
      let retryMs = 100;
      let isCancelled = false;

      const connect = () => {
        if (isCancelled) {
          return;
        }
        socket = new WebSocket(`${webSocketOrigin}/api/v1/events`);
        socket.addEventListener("open", () => {
          retryMs = 100;
        });
        socket.addEventListener("message", (message) => {
          let parsed: unknown;
          try {
            parsed = JSON.parse(message.data as string);
          } catch {
            return;
          }
          if (!isEventEnvelopeDto(parsed)) {
            return;
          }
          const event: EventEnvelope = mapEventEnvelope(parsed);
          if (shouldEmit(event.eventId)) {
            handlers.onEvent(event);
          }
        });
        socket.addEventListener("close", () => {
          if (isCancelled) {
            return;
          }
          const waitMs = retryMs;
          retryMs = Math.min(retryMs * 2, 2_000);
          retryTimer = window.setTimeout(connect, waitMs);
        });
        socket.addEventListener("error", () => {
          if (!isCancelled) {
            handlers.onError?.(new OperationsError("transport_error", "이벤트 스트림 연결에 실패했습니다."));
          }
        });
      };

      connect();
      return () => {
        isCancelled = true;
        if (retryTimer !== undefined) {
          window.clearTimeout(retryTimer);
        }
        if (socket?.readyState === WebSocket.OPEN || socket?.readyState === WebSocket.CONNECTING) {
          socket.close();
        }
      };
    },

    async listJobs() {
      const dto = await requestJson<ListJobsDto>("/api/v1/jobs");
      return dto.jobs.map(mapJobStatus);
    },

    async listJobsPage(cursor, highWatermark, limit) {
      const query = new URLSearchParams();
      if (cursor) query.set("cursor", cursor);
      if (highWatermark) query.set("high_watermark", highWatermark);
      if (limit !== undefined) query.set("limit", String(limit));
      const dto = await requestJson<JobPageDto>(`/api/v1/jobs/page?${query}`);
      const result: ResourcePage<JobStatus> = { records: dto.jobs.map(mapJobStatus), nextCursor: dto.next_cursor ?? undefined, highWatermark: dto.high_watermark, partial: dto.partial ?? false, failedPartitions: dto.failed_partitions ?? [] };
      return result;
    },

    async getJob(name) {
      return mapJobStatus(await requestJson<JobStatusDto>(`/api/v1/jobs/${encode(name)}`));
    },

    async addJob(config: JobConfigDto) {
      const dto = await requestJson<JobStatusDto>("/api/v1/jobs", {
        method: "POST",
        body: JSON.stringify(config),
      });
      return mapJobStatus(dto);
    },

    async updateJob(name, config) {
      return mapJobStatus(await requestJson<JobStatusDto>(`/api/v1/jobs/${encode(name)}`, { method: "PATCH", body: JSON.stringify(config) }));
    },

    async previewJob(request: JobPreviewRequestDto) {
      return mapJobPreview(await requestJson<JobPreviewDto>("/api/v1/jobs/preview", { method: "POST", body: JSON.stringify(request) }));
    },

    async removeJob(name, force = false) {
      const query = force ? "?force=true" : "";
      await requestNoContent(`/api/v1/jobs/${encode(name)}${query}`, { method: "DELETE" });
    },

    async triggerJob(name) {
      // 202 Accepted with a `{ run_id }` body (also Location header). Reading the
      // body is robust across origins where Location isn't an exposed header.
      const dto = await requestJson<TriggerJobDto>(`/api/v1/jobs/${encode(name)}/trigger`, {
        method: "POST",
      });
      return { runId: dto.run_id };
    },

    async listRuns(name, limit = 50) {
      const dto = await requestJson<ListRunsDto>(
        `/api/v1/jobs/${encode(name)}/runs?limit=${limit}`,
      );
      const result: JobRunsResult = {
        runs: dto.runs.map(mapJobRun),
        truncated: dto.truncated,
      };
      return result;
    },

    async getRun(name, runId) {
      return mapJobRun(await requestJson<JobRunDto>(`/api/v1/jobs/${encode(name)}/runs/${encode(runId)}`));
    },

    async cancelRun(name, runId) {
      await requestNoContent(`/api/v1/jobs/${encode(name)}/runs/${encode(runId)}/cancel`, { method: "POST" });
    },

    async daemonStatus() {
      const dto = await requestJson<DaemonStatusDto>("/api/v1/daemon/status");
      return mapDaemonStatus(dto);
    },
  } satisfies OperationsClient & { listProcesses(): Promise<ProcessStatus[]> };
}
