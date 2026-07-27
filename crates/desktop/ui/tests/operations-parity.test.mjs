import assert from "node:assert/strict";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";

const vite = await createServer({
  configFile: false,
  root: new URL("..", import.meta.url).pathname,
  server: { middlewareMode: true, hmr: false },
  optimizeDeps: { noDiscovery: true },
});

const jobResponse = {
  name: "local-defaults",
  trigger: { type: "cron", expr: "0 * * * *" },
  on_overlap: "skip",
  dependencies: { upstream: [], downstream: [] },
  timezone: "America/Los_Angeles",
  misfire_policy: "run_once",
};
const config = {
  name: "local-defaults",
  command: "/bin/true",
  trigger: { type: "cron", expr: "0 * * * *" },
};
const httpCalls = [];
const invokeCalls = [];

globalThis.fetch = async (url, init = {}) => {
  httpCalls.push({ url: String(url), method: init.method, body: init.body });
  if (String(url).endsWith("/api/v1/jobs")) {
    return new Response(JSON.stringify(jobResponse), { status: 201 });
  }
  return new Response(null, { status: String(url).endsWith("/restart") ? 202 : 204 });
};
globalThis.window = {
  __TAURI_INTERNALS__: {
    invoke: async (command, args) => {
      invokeCalls.push({ command, args });
      return command === "cmd_add_job" ? jobResponse : undefined;
    },
  },
};

try {
  const { createHttpClient } = await vite.ssrLoadModule("/src/services/http-client.ts");
  const { createInvokeClient } = await vite.ssrLoadModule("/src/services/invoke-client.ts");
  const { ProcessLifecycleActions, restartProcess } = await vite.ssrLoadModule("/src/features/processes/ProcessesView.tsx");
  const http = createHttpClient();
  const invoke = createInvokeClient();

  await http.startProcess("worker");
  await http.stopProcess("worker", true);
  await http.restartProcess("worker");
  await http.removeProcess("worker", false);
  await http.removeProcess("worker", true);
  const httpJob = await http.addJob(config);

  await invoke.startProcess("worker");
  await invoke.stopProcess("worker", true);
  await invoke.restartProcess("worker");
  await invoke.removeProcess("worker", false);
  await invoke.removeProcess("worker", true);
  const invokeJob = await invoke.addJob(config);

  assert.deepEqual(
    httpCalls.map(({ url, method }) => [new URL(url).pathname + new URL(url).search, method]),
    [
      ["/api/v1/processes/worker/start", "POST"],
      ["/api/v1/processes/worker/stop?force=true", "POST"],
      ["/api/v1/processes/worker/restart", "POST"],
      ["/api/v1/processes/worker", "DELETE"],
      ["/api/v1/processes/worker?force=true", "DELETE"],
      ["/api/v1/jobs", "POST"],
    ],
    "HTTP uses the documented lifecycle methods and force query flag",
  );
  assert.deepEqual(
    invokeCalls,
    [
      { command: "cmd_start_process", args: { name: "worker" } },
      { command: "cmd_stop_process", args: { name: "worker", force: true } },
      { command: "cmd_restart_process", args: { name: "worker" } },
      { command: "cmd_remove_process", args: { name: "worker", force: false } },
      { command: "cmd_remove_process", args: { name: "worker", force: true } },
      { command: "cmd_add_job", args: { config } },
    ],
    "Tauri uses the matching command names and camelCase arguments",
  );
  assert.deepEqual(httpJob, invokeJob, "both transports share JobStatus mapping");
  assert.equal(httpJob.timezone, jobResponse.timezone);
  assert.equal(httpJob.misfirePolicy, "run_once");
  assert.equal(httpCalls.at(-1).body, JSON.stringify(config), "GUI leaves Job defaults omitted");

  const restartCalls = [];
  const rollingRestartCalls = [];
  const removeCalls = [];
  const processActionsClient = {
    restartProcess: async (name) => { restartCalls.push(name); },
    rollingRestartProcess: async (name) => { rollingRestartCalls.push(name); },
    removeProcess: async (name, force) => { removeCalls.push([name, force]); },
  };
  const actionMarkup = renderToStaticMarkup(createElement(ProcessLifecycleActions, {
    client: processActionsClient,
    process: {
      name: "direct",
      state: "running",
      managementMode: { type: "direct" },
      pid: 1000,
      restartCount: 0,
      startedAt: null,
      cpuPercent: 0,
      memoryBytes: 0,
      uptime: "-",
    },
    pending: false,
    runAction: () => {},
  }));
  assert.match(actionMarkup, /aria-label="재시작"/, "ProcessesView renders the normal restart control");
  assert.match(actionMarkup, /aria-label="삭제"/, "ProcessesView renders the protected default delete control");
  assert.match(actionMarkup, /aria-label="강제 삭제"/, "ProcessesView renders a separate explicit force-delete control");
  assert.doesNotMatch(actionMarkup, /롤링 재시작/, "ProcessesView hides rolling restart until policy support is available to the UI");
  await restartProcess(processActionsClient, "direct");
  await restartProcess(processActionsClient, "system-registered");
  assert.deepEqual(restartCalls, ["direct", "system-registered"], "ProcessesView uses normal restart for Direct and SystemRegistered processes");
  assert.deepEqual(rollingRestartCalls, [], "ProcessesView does not silently issue a rolling restart");

  let lastAction = null;
  const lifecycleActions = ProcessLifecycleActions({
    client: processActionsClient,
    process: {
      name: "direct",
      state: "running",
      managementMode: { type: "direct" },
      pid: 1000,
      restartCount: 0,
      startedAt: null,
      cpuPercent: 0,
      memoryBytes: 0,
      uptime: "-",
    },
    pending: false,
    runAction: (name, action) => { lastAction = { name, action }; },
  });
  const actionButtons = lifecycleActions.props.children;
  const defaultDelete = actionButtons.find((button) => button.props.label === "삭제");
  const forceDelete = actionButtons.find((button) => button.props.label === "강제 삭제");
  const clickEvent = { stopPropagation() {} };

  defaultDelete.props.onClick(clickEvent);
  assert.equal(lastAction.name, "direct", "default delete routes through ProcessLifecycleActions");
  await lastAction.action();
  assert.deepEqual(removeCalls, [["direct", false]], "default delete preserves the running-process non-force conflict protection");

  window.confirm = () => true;
  forceDelete.props.onClick(clickEvent);
  assert.equal(lastAction.name, "direct", "confirmed force delete routes through ProcessLifecycleActions");
  await lastAction.action();
  assert.deepEqual(removeCalls, [["direct", false], ["direct", true]], "confirmed force delete preserves force=true");

  lastAction = null;
  window.confirm = () => false;
  forceDelete.props.onClick(clickEvent);
  assert.equal(lastAction, null, "cancelled force delete does not dispatch an action");
  assert.deepEqual(removeCalls, [["direct", false], ["direct", true]], "cancelled force delete does not call the client");
  console.log("desktop process/job HTTP-Tauri parity passed");
} finally {
  await vite.close();
}
