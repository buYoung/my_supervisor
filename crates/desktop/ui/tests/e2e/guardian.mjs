// A separate process group survives termination of the terminal/runner. The
// parent's IPC channel is the lifetime token; no unrelated PIDs are discovered.
import {writeFileSync, readFileSync, existsSync} from "node:fs";
import {join} from "node:path";
import {setTimeout as delay} from "node:timers/promises";
import {execFileSync} from "node:child_process";

const root = process.argv[2];
let groupId;
let isComplete = false;
process.on("message", message => {
  if (message.type === "group") groupId = message.pid;
  if (message.type === "complete") {isComplete = true; process.disconnect();}
});
process.on("disconnect", async () => {
  if (isComplete) return;
  const signals = [];
  if (groupId) {
    for (const signal of ["SIGTERM", "SIGKILL"]) {
      try {process.kill(-groupId, signal); signals.push(signal);} catch (error) {if (error.code === "ESRCH") break;}
      await delay(1000);
    }
  }
  const resourcesPath = join(root, "output/resources.json");
  const resources = existsSync(resourcesPath) ? JSON.parse(readFileSync(resourcesPath, "utf8")) : [];
  for (const {pid} of resources) {
    try {
      const state = execFileSync("ps", ["-p", String(pid), "-o", "stat="], {encoding: "utf8"});
      if (!state.trim().startsWith("Z")) process.kill(pid, "SIGTERM");
    } catch (error) {if (error.status !== 1 && error.code !== "ESRCH") throw error;}
  }
  writeFileSync(join(root, "output/guardian.json"), JSON.stringify({runnerDisconnected: true, ownedGroupId: groupId, signals, resources}, null, 2));
});
