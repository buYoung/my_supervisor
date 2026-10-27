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
  const { ProcessLifecycleActions, restartProcess, getProcessRemovalAction } = await vite.ssrLoadModule("/src/features/processes/ProcessesView.tsx");
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
  const stopCalls = [];
  const startCalls = [];
  const removeCalls = [];
  const processActionsClient = {
    startProcess: async (name) => { startCalls.push(name); },
    stopProcess: async (name) => { stopCalls.push(name); },
    restartProcess: async (name) => { restartCalls.push(name); },
    removeProcess: async (name, force) => { removeCalls.push([name, force]); },
  };
  const process = {
    name: "direct", state: "running", managementMode: {type: "direct"},
    pid: 1000, restartCount: 0, startedAt: null, cpuPercent: 0, memoryBytes: 0, uptime: "-",
  };
  let lastAction = null;
  const props = {client: processActionsClient, process, isBusy: false, onAction: action => {lastAction = action;}};
  const markup = renderToStaticMarkup(createElement(ProcessLifecycleActions, props));
  assert.match(markup, /aria-label="direct 중지"/, "stop is a visible primary control");
  assert.match(markup, /aria-label="direct 재시작"/, "normal restart is a visible primary control");
  assert.doesNotMatch(markup, /삭제|관리 방식/, "destructive and management actions are separate from runtime controls");
  const buttons = ProcessLifecycleActions(props).props.children;
  buttons[0].props.onClick(); await lastAction();
  buttons[1].props.onClick(); await lastAction();
  assert.deepEqual(stopCalls, ["direct"]);
  assert.deepEqual(restartCalls, ["direct"]);
  const stopped = {...process, state: "stopped"};
  const startMarkup = renderToStaticMarkup(createElement(ProcessLifecycleActions, {...props, process: stopped}));
  assert.match(startMarkup, /aria-label="direct 시작"/);
  assert.doesNotMatch(startMarkup, /aria-label="direct 재시작"/);
  ProcessLifecycleActions({...props, process: stopped}).props.children[0].props.onClick(); await lastAction();
  assert.deepEqual(startCalls, ["direct"]);
  await restartProcess(processActionsClient, "system-registered");
  assert.deepEqual(restartCalls, ["direct", "system-registered"], "restart uses the documented client operation");

  window.confirm = () => false;
  assert.equal(getProcessRemovalAction(processActionsClient, stopped), null, "stopped persisted removal can be cancelled");
  window.confirm = () => true;
  await getProcessRemovalAction(processActionsClient, stopped)();
  assert.deepEqual(removeCalls, [["direct", false]], "stopped process removal is not forced");
  window.confirm = () => false;
  assert.equal(getProcessRemovalAction(processActionsClient, process), null, "cancelled confirmation cannot dispatch removal");
  assert.deepEqual(removeCalls, [["direct", false]]);
  window.confirm = () => true;
  await getProcessRemovalAction(processActionsClient, process)();
  assert.deepEqual(removeCalls, [["direct", false], ["direct", true]], "active removal requires explicit confirmation and force");
  console.log("desktop process/job HTTP-Tauri parity passed");
} finally {
  await vite.close();
}
