import {appendFileSync, existsSync, readFileSync, writeFileSync} from "node:fs";
import {join} from "node:path";

const root = process.env.MSV_DESKTOP_E2E_ROOT;
if (!root || process.env.MSV_DESKTOP_E2E !== "1") throw new Error("Use pnpm test:native to allocate an isolated root");
const outputDir = join(root, "output");
const manifest=JSON.parse(readFileSync(join(outputDir,"manifest.json"),"utf8"));
const isOsObservation = process.env.MSV_E2E_OS_OBSERVE === "1";
let identity;

export const config = {
  runner: "local",
  specs: ["./tests/e2e/" + (process.env.MSV_E2E_SPEC ?? "harness.spec.mjs")],
  maxInstances: 1,
  capabilities: [{
    "tauri:options": {application: process.env.MSV_E2E_BINARY},
    "wdio:tauriServiceOptions": {
      driverProvider: "embedded",
      embeddedPort: Number(process.env.TAURI_WEBDRIVER_PORT),
      startTimeout: 20000,
      commandTimeout: 10000,
      captureBackendLogs: true,
      captureFrontendLogs: true,
      logLevel: "info",
    },
  }],
  services: [["@wdio/tauri-service", {driverProvider: "embedded"}]],
  framework: "mocha",
  reporters: ["spec"],
  logLevel: "warn",
  outputDir,
  waitforTimeout: 10000,
  connectionRetryTimeout: 15000,
  connectionRetryCount: 0,
  mochaOpts: {bail: true, timeout: isOsObservation ? 600000 : (manifest.command.includes("state.spec.mjs")||manifest.command.includes("flow.spec.mjs")||manifest.command.includes("visual.spec.mjs")) ? 120000 : 30000},
  async before() {
    await browser.waitUntil(() => browser.execute(() => Boolean(window.__MSV_E2E__)), {timeout: 10000});
    identity = await browser.execute(async () => ({
      revision: window.__MSV_E2E__.revision,
      sourceSha256: window.__MSV_E2E__.sourceSha256,
      platform: navigator.platform,
      webview: navigator.userAgent,
      url: location.href,
      transport: window.__MSV_E2E__.client.transport,
      daemon: await window.__MSV_E2E__.client.daemonStatus(),
    }));
    writeFileSync(join(outputDir, "identity.json"), JSON.stringify(identity, null, 2));
    if (isOsObservation) {
      const isolation=JSON.parse(readFileSync(join(outputDir,"os-isolation.json"),"utf8"));
      if(!isolation.virtualMachine || !isolation.model.startsWith("VirtualMac")) throw new Error("OS observation must stay inside a macOS VM");
      return;
    }
    await browser.waitUntil(() => existsSync(join(outputDir,"input-isolation.json")));
    const isolation=JSON.parse(readFileSync(join(outputDir,"input-isolation.json"),"utf8"));
    if(isolation.focused||isolation.applicationActive||isolation.opacity!==0||!isolation.ignoresMouseEvents) {
      writeFileSync(join(outputDir,"harness-error.json"),JSON.stringify({error:"Native input isolation failed",isolation}));
      throw new Error("Native input isolation failed");
    }
  },
  async afterTest(test, _context, {error, passed}) {
    const id = test.title.match(/(?:HARNESS|CMD|INPUT|SAVE|SCHEDULE|STATE|FLOW|LOG|VISUAL|GATE|ISOLATION)-\d+/)?.[0] ?? "unclassified";
    const screenshot = `${id}.png`;
    await browser.saveScreenshot(join(outputDir, screenshot)).catch(() => undefined);
    appendFileSync(join(outputDir, "results.jsonl"), JSON.stringify({
      id, title: test.title, mode: test.title.includes("[O]") ? "O" : test.title.includes("[N-M]") ? "N-M" : "N-R",
      passed, error: error?.message, identity, fixtureRoot: root, screenshot,
      mocks: id === "LOG-06" && !isOsObservation ? ["isolated in-memory clipboard; no system clipboard access"] : test.title.includes("[N-M]") ? [id === "HARNESS-02" ? "cmd_list_processes_page" : id.startsWith("VISUAL") ? "listProcessesPage presentation/failure fixture" : "cmd_update_process_config"] : [],
      command: manifest.command, cwd: manifest.cwd,
    }) + "\n");
    await browser.tauri.restoreAllMocks();
    await browser.execute(() => window.__MSV_E2E__.restoreInvoke());
    const isolation=await browser.execute(async()=>({focused:await window.__TAURI__.window.getCurrentWindow().isFocused(),nativeKeyEvents:window.__MSV_E2E__.nativeKeyEventCount}));
    appendFileSync(join(outputDir,"input-isolation-checks.jsonl"),JSON.stringify({id,...isolation})+"\n");
    if(!isOsObservation && (isolation.focused||isolation.nativeKeyEvents!==0))writeFileSync(join(outputDir,"harness-error.json"),JSON.stringify({error:"Desktop input reached automatic test window",isolation}));
  },
  async beforeTest() {
    if (!await browser.execute(() => window.__MSV_E2E__.isInvokeRestored())) {
      writeFileSync(join(outputDir, "harness-error.json"), JSON.stringify({error: "Native scenario started with an active invoke mock"}));
      throw new Error("Native scenario started with an active invoke mock");
    }
  },
  async after() {
    try {await browser.tauri.restoreAllMocks();}
    finally {
      await browser.execute(async () => {
        const harness = window.__MSV_E2E__;
        harness.restoreInvoke();
        for (const process of await harness.client.listProcesses()) {
          if (process.state === "running" || process.state === "starting") await harness.client.stopProcess(process.name);
        }
      });
    }
  },
};
