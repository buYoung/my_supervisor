import assert from "node:assert/strict";
import { createServer } from "vite";

const vite = await createServer({
  configFile: false,
  root: new URL("..", import.meta.url).pathname,
  server: { middlewareMode: true, hmr: false },
  optimizeDeps: { noDiscovery: true },
});

const received = [];
const eventA = { type: "job.run_succeeded", event_id: "A", timestamp: "2026-07-14T00:00:00Z", payload: { run_id: "run-a" } };
const eventB = { type: "job.run_failed", event_id: "B", timestamp: "2026-07-14T00:00:01Z", payload: { run_id: "run-b" } };

class FakeWebSocket {
  static OPEN = 1;
  static CONNECTING = 0;
  static instances = [];

  constructor() {
    this.readyState = FakeWebSocket.OPEN;
    this.listeners = new Map();
    FakeWebSocket.instances.push(this);
  }

  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }

  close() {
    this.readyState = 3;
    this.emit("close", {});
  }

  emit(type, event) {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

globalThis.window = {
  setTimeout(callback) {
    queueMicrotask(callback);
    return 1;
  },
  clearTimeout() {},
};
globalThis.WebSocket = FakeWebSocket;

try {
  const { createHttpClient } = await vite.ssrLoadModule("/src/services/http-client.ts");
  const { createInvokeClient } = await vite.ssrLoadModule("/src/services/invoke-client.ts");
  const { createEventDeduper } = await vite.ssrLoadModule("/src/services/operations-client.ts");

  const shouldEmitFromBoundedCache = createEventDeduper(2);
  assert.deepEqual(
    ["A", "A", "B", "C", "A"].filter((eventId) => shouldEmitFromBoundedCache(eventId)),
    ["A", "B", "C", "A"],
    "dedupe cache bounds retention and accepts an evicted event ID",
  );

  const httpClient = createHttpClient();
  const stopHttp = httpClient.followEvents({ onEvent: (event) => received.push(`http:${event.eventId}`) });
  const firstSocket = FakeWebSocket.instances.at(-1);
  firstSocket.emit("message", { data: JSON.stringify(eventA) });
  firstSocket.emit("message", { data: JSON.stringify(eventA) });
  firstSocket.emit("message", { data: JSON.stringify(eventB) });
  firstSocket.emit("close", {});
  await new Promise((resolve) => setImmediate(resolve));
  FakeWebSocket.instances.at(-1).emit("message", { data: JSON.stringify(eventA) });
  stopHttp();
  assert.deepEqual(received, ["http:A", "http:B"], "HTTP drops duplicate and reconnect replay");

  let globalEventListener;
  const invokeClient = createInvokeClient(async () => async (name, listener) => {
    assert.equal(name, "global-event");
    globalEventListener = listener;
    return () => {};
  });
  const invokeEvents = [];
  const stopInvoke = invokeClient.followEvents({ onEvent: (event) => invokeEvents.push(event.eventId) });
  await new Promise((resolve) => setImmediate(resolve));
  globalEventListener({ payload: eventA });
  globalEventListener({ payload: eventA });
  globalEventListener({ payload: eventB });
  globalEventListener({ payload: eventA });
  stopInvoke();
  assert.deepEqual(invokeEvents, ["A", "B"], "Tauri drops [A,A,B] and replay A");
  console.log("desktop followEvents HTTP/Tauri dedupe passed");
} finally {
  await vite.close();
}
