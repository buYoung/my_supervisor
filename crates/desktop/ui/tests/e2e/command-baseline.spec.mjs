import assert from "node:assert/strict";
import {writeFileSync, existsSync} from "node:fs";
import {join} from "node:path";

describe("Command baseline", () => {
  it("CMD-04 [N-R] records unsolicited execution before the fix", async () => {
    const root = process.env.MSV_DESKTOP_E2E_ROOT;
    const sentinel = join(root, "sentinel.sh");
    const marker = join(root, "output/sentinel-executed");
    writeFileSync(sentinel, `#!/bin/sh\nprintf 'executed\\n' >> '${marker}'\nprintf 'Usage: sentinel [--help]\\n'\n`, {mode: 0o700});
    await browser.execute(async program => {
      const h = window.__MSV_E2E__;
      await h.client.addProcess(h.processFixture("native-sentinel", program, ["original"]));
    }, sentinel);
    await $("button[aria-label='native-sentinel 더 보기']").waitForExist();
    await $("button[aria-label='native-sentinel 더 보기']").click();
    await $(".overflow-menu button").click();
    await browser.waitUntil(() => existsSync(marker), {timeout: 6000});
    assert.equal(existsSync(marker), true);
    writeFileSync(join(root, "output/command-baseline.json"), JSON.stringify({automaticSentinelExecution: true, program: sentinel, originalArgs: ["original"]}, null, 2));
    await $("button=취소").click();
  });

  it("CMD-03 [N-M] records invalid text submitting the previous argv", async () => {
    await $("button[aria-label='native-sentinel 더 보기']").click();
    await $(".overflow-menu button").click();
    const input = await $("input[aria-label='전체 실행 명령 인수']");
    await input.setValue("'unclosed");
    await browser.execute(() => document.querySelector("input[aria-label='전체 실행 명령 인수']").blur());
    await $(".command-editor [role='alert']").waitForExist();
    await browser.execute(() => window.__MSV_E2E__.enableMockRouting());
    const mock = await browser.tauri.mock("cmd_update_process_config");
    const status = await browser.execute(() => window.__MSV_E2E__.processStatusFixture("native-sentinel"));
    await mock.mockReturnValue(status);
    const enabled = await $("button=설정 적용").isEnabled();
    await $("button=설정 적용").click();
    assert.equal(enabled, true);
    assert.ok(mock.mock.calls.length > 0);
    const args = mock.mock.calls[0][0];
    assert.deepEqual(args.request.config.args, ["original"]);
    writeFileSync(join(process.env.MSV_DESKTOP_E2E_ROOT, "output/invalid-baseline.json"), JSON.stringify({invalidApplyEnabled: enabled, submittedArgs: args.request.config.args, mocks: ["cmd_update_process_config"]}, null, 2));
  });
});
