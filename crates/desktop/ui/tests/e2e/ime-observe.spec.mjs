import assert from "node:assert/strict";
import {verifyNativeIme} from "./verify-native-ime.mjs";
import {existsSync,readFileSync,writeFileSync} from "node:fs";
import {join} from "node:path";
import {setTimeout as delay} from "node:timers/promises";

describe("Native Korean input observation",()=>{
  it("INPUT-07 [O] native composition confirms text without adding a row or saving",async function(){
    this.timeout(300000);
    const root=process.env.MSV_DESKTOP_E2E_ROOT;
    await browser.execute(async()=>{
      const h=window.__MSV_E2E__;
      await h.client.addJob(h.jobFixture("한글"));await h.client.addJob(h.jobFixture("native-ime"));
    });
    await $("button=작업").click();await $(".job-table-row*=native-ime").waitForExist();await $(".job-table-row*=native-ime").click();await $("button=흐름 편집").click();await $("button=작업 설정").waitForExist();await $("button=작업 설정").click();await $(".schedule-fields select").waitForExist();
    await browser.execute(()=>{const select=document.querySelector(".schedule-fields select");select.value="depends_on";select.dispatchEvent(new Event("change",{bubbles:true}));});
    await $("input[aria-label='선행 작업 1']").click();
    await browser.execute(()=>window.__MSV_E2E__.beginNativeInputObservation());
    writeFileSync(join(root,"output/observation-ready.json"),JSON.stringify({appPid:Number(readFileSync(join(root,"app.pid"),"utf8")),field:"선행 작업 1",required:"Use native Korean input, press Return once to confirm composition, inspect, then press Return again to add the row"},null,2));
    const done=join(root,"output/observation-done.json");
    const firstEnter=join(root,"output/ime-first-enter-result.json");
    while(!existsSync(done)) {
      if(existsSync(join(root,"output/ime-first-enter-request.json"))&&!existsSync(firstEnter)) {
        const state=await browser.execute(()=>({rowCount:document.querySelectorAll(".repeated-row").length,values:[...document.querySelectorAll(".repeated-row input")].map(input=>input.value),active:document.activeElement?.getAttribute("aria-label")}));
        writeFileSync(firstEnter,JSON.stringify(state,null,2));
        await browser.saveScreenshot(join(root,"output/ime-first-enter.png"));
      }
      await delay(100);
    }
    const events=await browser.execute(()=>window.__MSV_E2E__.nativeInputEvents);
    const result=await browser.execute(()=>({rowCount:document.querySelectorAll(".repeated-row").length,values:[...document.querySelectorAll(".repeated-row input")].map(input=>input.value),active:document.activeElement?.getAttribute("aria-label")}));
    const actions=JSON.parse(readFileSync(done,"utf8"));
    if(actions.method==="vm-hardware-keyboard")actions.firstEnter=JSON.parse(readFileSync(firstEnter,"utf8"));
    writeFileSync(join(root,"output/native-ime.json"),JSON.stringify({actions,events,result},null,2));
    await browser.saveScreenshot(join(root,"output/native-ime.png"));
    verifyNativeIme({events,result,actions});
    const saved=await browser.execute(()=>window.__MSV_E2E__.client.getJobConfig("native-ime"));assert.equal(saved.trigger.type,"one_shot");
  });
});
