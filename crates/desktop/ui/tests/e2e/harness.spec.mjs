import assert from "node:assert/strict";
import {writeFileSync} from "node:fs";
import {join} from "node:path";

describe("Isolated native host", () => {
  it("HARNESS-01 [N-R] real main window and invoke persistence", async () => {
    assert.equal(await browser.execute(() => window.__MSV_E2E__.windowLabel), "main");
    const result = await browser.execute(async () => {
      const {client, processFixture} = window.__MSV_E2E__;
      const config = processFixture("native-smoke", "/usr/bin/printf", ["%s", "한글, literal", ""]);
      await client.addProcess(config);
      await client.addJob(window.__MSV_E2E__.jobFixture("native-dependency"));
      await client.addProcess(processFixture("native-command-order", "/usr/bin/printf", ["run", "start", "--", "--port", "3000"]));
      return {status: await client.daemonStatus(), config: await client.getProcessConfig(config.name), transport: client.transport};
    });
    assert.equal(result.transport, "invoke");
    assert.equal(result.status.configPath, join(process.env.MSV_DESKTOP_E2E_ROOT, "config/config.toml"));
    assert.equal(result.status.logDir, join(process.env.MSV_DESKTOP_E2E_ROOT, "logs"));
    assert.deepEqual(result.config.args, ["%s", "한글, literal", ""]);
    assert.ok(result.config.definition_id);
    assert.ok(result.status.pid > 0);
    await browser.waitUntil(async () => (await $("body").getText()).includes("native-smoke"));
    const running = await browser.execute(async () => {
      const {client, processFixture} = window.__MSV_E2E__;
      await client.addProcess(processFixture("native-owned-sleeper", "/bin/sleep", ["60"]));
      await client.startProcess("native-owned-sleeper");
      return client.getProcess("native-owned-sleeper");
    });
    assert.equal(running.state, "running");
    assert.ok(running.pid > 0);
    writeFileSync(join(process.env.MSV_DESKTOP_E2E_ROOT, "output/resources.json"), JSON.stringify([{name: running.name, pid: running.pid}]));
    if (process.env.MSV_E2E_PROBE === "failure") assert.fail("deliberate cleanup probe");
    if (process.env.MSV_E2E_PROBE === "interrupt") {
      writeFileSync(join(process.env.MSV_DESKTOP_E2E_ROOT, "ready-to-interrupt"), "ready");
      await browser.pause(20000);
    }
  });

  it("HARNESS-02 [N-M] module invoke rejection and complete restoration", async () => {
    await browser.execute(() => window.__MSV_E2E__.enableMockRouting());
    const mock = await browser.tauri.mock("cmd_list_processes_page");
    await mock.mockRejectedValue("native controlled rejection");
    await browser.waitUntil(async () => (await $("body").getText()).includes("native controlled rejection"));
    await browser.tauri.restoreAllMocks();
    await browser.execute(() => window.__MSV_E2E__.restoreInvoke());
    assert.equal(await browser.execute(() => window.__MSV_E2E__.isInvokeRestored()), true);
    const config = await browser.execute(() => window.__MSV_E2E__.client.getProcessConfig("native-smoke"));
    assert.deepEqual(config.args, ["%s", "한글, literal", ""]);
    await browser.waitUntil(async () => !(await $("body").getText()).includes("native controlled rejection"));
  });

  it("HARNESS-04 [N-R] records integrated native layout and input capabilities", async () => {
    const initial = await browser.getWindowSize();
    await browser.setWindowSize(1100, 720);
    const result = await browser.execute(() => ({
      title: document.title, width: innerWidth, height: innerHeight,
      columns: getComputedStyle(document.querySelector(".process-list-head")).gridTemplateColumns,
      focusedTag: document.activeElement?.tagName,
      restored: window.__MSV_E2E__.isInvokeRestored(),
    }));
    assert.equal(result.restored, true);
    assert.ok(result.width > 0);
    await $("button[aria-label='native-command-order 더 보기']").waitForExist();
    await $("button[aria-label='native-command-order 더 보기']").click();
    await $(".overflow-menu button").click();
    const input = await $("input[aria-label='전체 실행 명령 인수']");
    await input.click();
    await input.setValue("run start -- --port 3000");
    await browser.execute(() => document.querySelector("input[aria-label='전체 실행 명령 인수']").blur());
    await browser.waitUntil(async () => (await $$(".argument-row")).length > 0);
    const baseline = await $$(".argument-row").map(async row => ({
      name: await row.$("input[aria-label='옵션 이름']").getValue(),
      value: await row.$("input[aria-label='옵션 값']").getValue(),
    }));
    const values = await $$(".argument-row input[aria-label='옵션 값']");
    await values[values.length - 1].setValue("4000");
    await browser.waitUntil(async () => (await input.getValue()).includes("4000"));
    const afterPortEdit = await input.getValue();
    await $("button=취소").click();
    if(await $("button=변경 버리기").isExisting())await $("button=변경 버리기").click();
    await $("button=작업").click();
    await $("button=작업 추가").click();
    await browser.execute(() => {
      const select = document.querySelector(".schedule-fields select");
      select.value = "depends_on";
      select.dispatchEvent(new Event("change", {bubbles: true}));
    });
    const dependency = await $("input[aria-label='선행 작업 1']");
    await dependency.waitForExist();
    await dependency.setValue("native-dependency");
    await browser.keys(",");
    const delimiterDraft = await dependency.getValue();
    assert.equal(afterPortEdit,"run start -- --port 4000");
    assert.equal(delimiterDraft,"native-dependency");
    assert.equal(await browser.execute(()=>document.activeElement.getAttribute("aria-label")),"선행 작업 2");
    assert.ok(Number.parseFloat(result.columns.split(" ").at(-1)) >= 200);
    writeFileSync(join(process.env.MSV_DESKTOP_E2E_ROOT, "output/integrated-baseline.json"), JSON.stringify({
      layout: result, commandRows: baseline, afterPortEdit, delimiterDraft,
      correctedObservations: [
        {id: "CMD-01", expected: "run start -- --port 4000", actual: afterPortEdit},
        {id: "INPUT-01", expected: "native-dependency selected, next row focused", actual: delimiterDraft},
        {id: "VISUAL-01", expected: "at least 200px actions column", actual: result.columns},
      ],
      limitations: ["WebDriver text input is not native Korean IME proof", "Native file dialog, clipboard and close-to-tray require OS observations"],
      initial,
    }, null, 2));
    await $("button=취소").click();
    if(await $("button=변경 버리기").isExisting())await $("button=변경 버리기").click();
  });
});
