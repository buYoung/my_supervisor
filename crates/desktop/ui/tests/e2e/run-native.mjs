import {spawn, fork, execFileSync} from "node:child_process";
import {mkdtempSync, mkdirSync, realpathSync, writeFileSync, readFileSync, existsSync, createWriteStream} from "node:fs";
import {tmpdir} from "node:os";
import {join, resolve, dirname} from "node:path";
import {fileURLToPath} from "node:url";
import net from "node:net";
import {setTimeout as delay} from "node:timers/promises";
import {createHash} from "node:crypto";

const repository = resolve(dirname(fileURLToPath(import.meta.url)), "../../../../..");
const ui = join(repository, "crates/desktop/ui");
const options = process.argv.slice(2);
const isOsObservation = options.includes("--os-observe");
if (isOsObservation && !execFileSync("/usr/sbin/sysctl", ["-n", "hw.model"], {encoding: "utf8"}).trim().startsWith("VirtualMac")) throw new Error("OS observation runner requires a separate macOS virtual machine");
const sourceManifest = isOsObservation && process.env.MSV_E2E_SOURCE_MANIFEST ? JSON.parse(readFileSync(process.env.MSV_E2E_SOURCE_MANIFEST, "utf8")) : undefined;
const target = join(repository, isOsObservation ? "target/desktop-e2e-os" : "target/desktop-e2e");
const revision = sourceManifest?.revision ?? execFileSync("git", ["rev-parse", "HEAD"], {cwd: repository, encoding: "utf8"}).trim();
const hash = createHash("sha256");
const sourceFiles = sourceManifest?.sourceFiles ?? execFileSync("git", ["ls-files", "--cached", "--others", "--exclude-standard", "-z", "crates", "Cargo.toml", "Cargo.lock", "scripts/build-local.sh"], {cwd: repository, encoding: "utf8"}).split("\0").filter(path => path && !path.startsWith("crates/desktop/gen/schemas/")).sort();
for (const path of new Set(sourceFiles)) {hash.update(path); hash.update(readFileSync(join(repository, path)));}
const sourceSha256 = hash.digest("hex");
if (sourceManifest && sourceManifest.sourceSha256 !== sourceSha256) throw new Error("Copied OS observation sources do not match the build manifest");
const probe = options.find(value => value.startsWith("--probe="))?.split("=")[1] ?? "normal";
const spec = options.find(value => value.startsWith("--spec="))?.slice(7) ?? "harness.spec.mjs";
if (options.includes("--activate") || options.includes("--observe") || (!isOsObservation && ["ime-observe.spec.mjs","native-gates.spec.mjs","visual-baseline.spec.mjs"].includes(spec))) throw new Error("Shared-desktop OS observation is disabled; use --os-observe only inside a macOS virtual machine");
const scenario = options.find(value => value.startsWith("--case="))?.slice(7);
if (scenario && !/^[A-Z]+-\d+$/.test(scenario)) throw new Error("Invalid native scenario selector");
if (!/^[a-z0-9-]+\.spec\.mjs$/.test(spec)) throw new Error("Native specs must be local named E2E specs");
if (!["normal", "failure", "startup", "interrupt"].includes(probe)) throw new Error("Unknown cleanup probe");
const root = realpathSync(mkdtempSync(join(tmpdir(), "msv-desktop-ux-")));
for (const name of ["config", "data", "logs", "output"]) mkdirSync(join(root, name));
writeFileSync(join(root, "fixture-owner.json"), JSON.stringify({revision, createdAt: new Date().toISOString(), runnerPid: process.pid}));
writeFileSync(join(root, "config/config.toml"), "# Isolated native UX fixtures; no autostart entries.\n");
const port = await new Promise((resolvePort, reject) => {
  const server = net.createServer();
  server.once("error", reject);
  server.listen(0, "127.0.0.1", () => {
    const value = server.address().port;
    server.close(() => resolvePort(value));
  });
});
const env = {...process.env, CARGO_TARGET_DIR: target, MSV_E2E_REVISION: revision,
  MSV_E2E_SOURCE_SHA256: sourceSha256,
  MSV_E2E_OS_OBSERVE: isOsObservation ? "1" : "0",
  MSV_DESKTOP_E2E: "1", MSV_DESKTOP_E2E_ROOT: root, MSV_E2E_PROBE: probe,
  MSV_E2E_SPEC: spec,
  MSV_E2E_BINARY: join(target, "debug/bundle/macos/my-supervisor.app/Contents/MacOS/my-supervisor"),
  TAURI_WEBDRIVER_PORT: String(port), TZ: "Asia/Seoul"};
delete env.MSV_UI_DATA_DIR;
delete env.MSV_UI_API_URL;
delete env.MSV_DEVBRIDGE_PORT;
delete env.MSV_DAEMON_TEST_DATA_DIR;
const manifest = {revision, sourceSha256, root, port, probe, isOsObservation, binary: env.MSV_E2E_BINARY, cwd: ui,
  command: ["pnpm", "test:native", ...options].join(" ")};
writeFileSync(join(root, "output/manifest.json"), JSON.stringify(manifest, null, 2));
console.log(`Native evidence: ${root}/output`);
const guardian = fork(join(dirname(fileURLToPath(import.meta.url)), "guardian.mjs"), [root], {detached: true, stdio: ["ignore", "ignore", "ignore", "ipc"]});
let active;
let interrupted = false;
function stopOwnedGroup(signal = "SIGTERM") {
  if (active?.pid) {
    try {process.kill(-active.pid, signal);} catch (error) {if (error.code !== "ESRCH") throw error;}
  }
}
for (const signal of ["SIGINT", "SIGTERM"]) process.once(signal, () => {interrupted = true; stopOwnedGroup();});
async function run(command, args, cwd, logName, overrides = {}) {
  const stream = createWriteStream(join(root, "output", logName));
  active = spawn(command, args, {cwd, env: {...env, ...overrides}, detached: true, stdio: ["ignore", "pipe", "pipe"]});
  guardian.send({type: "group", pid: active.pid});
  for (const pipe of [active.stdout, active.stderr]) pipe.on("data", value => {process.stdout.write(value); stream.write(value);});
  const timeout = setTimeout(() => {interrupted = true; stopOwnedGroup();}, logName === "build.log" || isOsObservation ? 600000 : 120000);
  const code = await new Promise((resolveCode, reject) => {
    active.once("error", reject);
    active.once("exit", (code, signal) => resolveCode(code ?? (signal ? 130 : 1)));
  }).finally(() => {clearTimeout(timeout); stream.end();});
  stopOwnedGroup();
  active = undefined;
  guardian.send({type: "group", pid: undefined});
  return code;
}
let exitCode = 1;
function isRunning(pid) {
  try {return !execFileSync("ps", ["-p", String(pid), "-o", "stat="], {encoding: "utf8", stdio: ["ignore", "pipe", "ignore"]}).trim().startsWith("Z");}
  catch (error) {if (error.status === 1) return false; throw error;}
}
try {
  if (!options.includes("--no-build")) {
    const helpers = await run("bash", ["scripts/build-local.sh", "cli", "debug"], repository, "helpers.log");
    if (helpers !== 0) throw new Error(`Helper build failed: ${helpers}`);
    const build = await run("cargo", ["tauri", "build", "--debug", "--no-sign", "--bundles", "app", "--features", isOsObservation ? "desktop-e2e-os" : "desktop-e2e", "--config", isOsObservation ? "tauri.e2e-os.conf.json" : "tauri.e2e.conf.json"], join(repository, "crates/desktop"), "build.log");
    if (build !== 0) throw new Error(`Native build failed: ${build}`);
  }
  if (!existsSync(env.MSV_E2E_BINARY)) throw new Error(`Build missing: ${env.MSV_E2E_BINARY}`);
  manifest.binarySha256 = createHash("sha256").update(readFileSync(env.MSV_E2E_BINARY)).digest("hex");
  manifest.sourceHashScope = "Git tracked and untracked crates, Cargo files and build-local.sh, excluding derived desktop/gen/schemas";
  writeFileSync(join(root, "output/manifest.json"), JSON.stringify(manifest, null, 2));
  let interruptTimer;
  if (probe === "interrupt") interruptTimer = setInterval(() => {
    if (existsSync(join(root, "ready-to-interrupt"))) {interrupted = true; stopOwnedGroup("SIGINT"); clearInterval(interruptTimer);}
  }, 100);
  try {
    const args = ["exec", "wdio", "run", "wdio.conf.mjs"];
    if (probe === "failure" || probe === "interrupt") args.push("--mochaOpts.grep=HARNESS-01");
    else if (scenario) args.push("--mochaOpts.grep=" + scenario);
    const code = await run("pnpm", args, ui, "native.log");
    const log = readFileSync(join(root, "output/native.log"), "utf8");
    const expectedFailure = code !== 0 && (probe === "startup" ? log.includes("code=101")
      : probe === "failure" ? log.includes("deliberate cleanup probe")
        : probe === "interrupt" && existsSync(join(root, "ready-to-interrupt")));
    writeFileSync(join(root, "output/exit.json"), JSON.stringify({probe, code, expectedFailure}, null, 2));
    exitCode = probe === "normal" ? code : expectedFailure ? 0 : 1;
    const resultsFile = join(root, "output/results.jsonl");
    if (probe === "normal" && (existsSync(join(root, "output/harness-error.json")) || !existsSync(join(root, "output/identity.json")) || !existsSync(resultsFile) || !readFileSync(resultsFile, "utf8").trim())) exitCode = 1;
  } finally {clearInterval(interruptTimer);}
} catch (error) {
  console.error(error);
  writeFileSync(join(root, "output/error.txt"), String(error));
} finally {
  stopOwnedGroup();
  await delay(500);
  const httpFile = join(root, "output/native-http.json");
  const httpPort = existsSync(httpFile) ? JSON.parse(readFileSync(httpFile,"utf8")).port : undefined;
  const httpPortClosed = !httpPort || await new Promise(resolveClosed=>{const socket=net.connect({port:httpPort,host:"127.0.0.1"});socket.once("connect",()=>{socket.destroy();resolveClosed(false);});socket.once("error",()=>resolveClosed(true));});
  const portClosed = await new Promise(resolveClosed => {
    const socket = net.connect({port, host: "127.0.0.1"});
    socket.once("connect", () => {socket.destroy(); resolveClosed(false);});
    socket.once("error", () => resolveClosed(true));
  });
  let appStopped = true;
  const pidFile = join(root, "app.pid");
  if (existsSync(pidFile)) {
    try {process.kill(Number(readFileSync(pidFile, "utf8")), 0); appStopped = false;} catch (error) {if (error.code !== "ESRCH") appStopped = false;}
  }
  const resourceFile = join(root, "output/resources.json");
  const resources = existsSync(resourceFile) ? JSON.parse(readFileSync(resourceFile, "utf8")) : [];
  const ownedProcesses = [];
  for (const resource of resources) {
    const requiredCleanup = isRunning(resource.pid);
    if (requiredCleanup) {
      try {process.kill(resource.pid, "SIGTERM");} catch (error) {if (error.code !== "ESRCH") throw error;}
      await delay(500);
      if (isRunning(resource.pid)) process.kill(resource.pid, "SIGKILL");
    }
    ownedProcesses.push({...resource, requiredCleanup, stopped: !isRunning(resource.pid)});
  }
  writeFileSync(join(root, "output/cleanup.json"), JSON.stringify({portClosed, httpPortClosed, appStopped, ownedProcesses, interrupted, probe, retainedFixtureRoot: root}, null, 2));
  if (!portClosed || !httpPortClosed || !appStopped || ownedProcesses.some(resource => !resource.stopped)) exitCode = 1;
  if (interrupted && probe !== "interrupt") exitCode = 130;
  const exitFile = join(root, "output/exit.json");
  if (existsSync(exitFile)) {
    const result = JSON.parse(readFileSync(exitFile, "utf8"));
    writeFileSync(exitFile, JSON.stringify({...result, runnerCode: exitCode}, null, 2));
  }
  console.log(`Native evidence: ${root}/output; cleanup: ${portClosed && httpPortClosed && appStopped && ownedProcesses.every(resource => resource.stopped) ? "PASS" : "FAIL"}`);
  guardian.send({type: "complete"});
}
process.exitCode = exitCode;
