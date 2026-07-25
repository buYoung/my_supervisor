/**
 * Adapter selection + public surface of the services layer.
 *
 * Runtime selection: inside Tauri (the production desktop app) the invoke adapter is used;
 * everywhere else (standalone browser / vite dev) the HTTP+WS adapter talks to the daemon.
 * Detection is by the Tauri-injected globals; nothing transport-specific leaks into the views.
 */

import { createHttpClient } from "./http-client";
import { createInvokeClient } from "./invoke-client";
import type { OperationsClient } from "./operations-client";

export { OperationsError } from "./operations-client";
export type {
  EventEnvelope,
  FollowEventsHandlers,
  FollowLogsHandlers,
  JobRunsResult,
  OperationsClient,
  ProcessLogsTail,
} from "./operations-client";

/** True when running inside a Tauri webview (the production path). */
export function isTauriRuntime(): boolean {
  if (typeof window === "undefined") {
    return false;
  }
  return "__TAURI_INTERNALS__" in window || "__TAURI__" in window;
}

let cachedClient: OperationsClient | null = null;

/** The active operations client for this runtime — invoke inside Tauri, HTTP otherwise. */
export function getOperationsClient(): OperationsClient {
  if (!cachedClient) {
    cachedClient = isTauriRuntime() ? createInvokeClient() : createHttpClient();
  }
  return cachedClient;
}
