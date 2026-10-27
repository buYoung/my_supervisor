import {writeFileSync} from "node:fs";
import {join} from "node:path";

async function select(selector,value){await browser.execute(([selector,value])=>{const input=document.querySelector(selector);input.value=value;input.dispatchEvent(new Event("change",{bubbles:true}));},[selector,value]);}
describe("Input baseline",()=>{
  it("INPUT-04 [N-R] records blank number, policy and environment draft behavior",async()=>{
    await browser.execute(async()=>{
      const h=window.__MSV_E2E__;
      await h.client.addProcess({...h.processFixture("native-input"),env:{DATABASE:"base"},liveness:{kind:{type:"http",url:"http://127.0.0.1:8123/custom",expected_status:200},interval_ms:5000,timeout_ms:3000,consecutive_successes:1,consecutive_failures:3}});
      await h.client.addJob({...h.jobFixture("native-interval"),trigger:{type:"interval",everySec:120}});
    });
    await $("button[aria-label='native-input 더 보기']").waitForExist();
    await $("button[aria-label='native-input 더 보기']").click();await $(".overflow-menu button").click();
    await $("button=관리·종료").click();
    const instances=await $(".management-section input[type='number']");await instances.clearValue();
    const blankNumber=await instances.getValue();
    await $("button=상태 확인").click();
    await $(".guard-controls .policy-toggle input").click();await $(".guard-controls .policy-toggle input").click();
    const resetUrl=await $(".guard-controls .editor-columns input").getValue();
    await $("button=환경변수").click();await $("input[aria-label='환경변수 검색']").setValue("DATABASE");
    await $("button=추가").click();
    const visibleKeys=await $$("input[aria-label='환경변수 이름']").map(input=>input.getValue());
    await $("input[aria-label='환경변수 이름']").setValue("bad-key");
    await browser.execute(()=>document.querySelector("input[aria-label='환경변수 이름']").blur());
    const invalidKey=await $("input[aria-label='환경변수 이름']").getValue();
    await $("button=취소").click();
    await $("button=작업").click();await $(".job-table-row*=native-interval").waitForExist();
    await $(".job-table-row*=native-interval").click();await $("button=흐름 편집").click();await $("button=작업 설정").waitForExist();await $("button=작업 설정").click();
    await select(".schedule-fields select","cron");await select(".schedule-fields select","interval");
    const intervalAfterSwitch=await $(".schedule-fields input[type='number']").getValue();
    writeFileSync(join(process.env.MSV_DESKTOP_E2E_ROOT,"output/input-baseline.json"),JSON.stringify({blankNumber,resetUrl,visibleKeys,invalidKey,intervalAfterSwitch},null,2));
    await $("dialog .editor-footer button").click();
  });
});
