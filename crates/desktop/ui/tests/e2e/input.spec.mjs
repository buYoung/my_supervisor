import assert from "node:assert/strict";
import {writeFileSync,mkdirSync} from "node:fs";
import {join} from "node:path";
import {createServer} from "node:http";

const root=process.env.MSV_DESKTOP_E2E_ROOT;
const commaPath=join(root,"folder, name");const secondPath=join(root,"second");
async function select(selector,value){await browser.execute(([selector,value])=>{const input=document.querySelector(selector);input.value=value;input.dispatchEvent(new Event("change",{bubbles:true}));},[selector,value]);}
async function openProcess(){await $("button=프로세스").click();await $("button[aria-label='native-input 더 보기']").waitForExist();await $("button[aria-label='native-input 더 보기']").click();await $(".overflow-menu button").click();}
async function openJob(name){await $("button=작업").click();if(await $("button=작업 목록").isExisting())await $("button=작업 목록").click();await $(`.job-table-row*=${name}`).waitForExist();await $(`.job-table-row*=${name}`).click();await $("button=흐름 편집").click();await $("button=작업 설정").waitForExist();await $("button=작업 설정").click();await $(".schedule-fields select").waitForExist();}
async function saveProcess(){await $('dialog').$('button=실행').click();assert.equal(await browser.execute(()=>document.querySelector('dialog .editor-body .editor-field input').value),'native-input','Input fixture name must remain unchanged');await $("button=설정 적용").click();await $("dialog").waitForExist({reverse:true});}
async function saveJob(){await $("dialog .editor-footer button:last-child").click();await $("dialog").waitForExist({reverse:true});}
async function config(){return browser.execute(()=>window.__MSV_E2E__.client.getProcessConfig("native-input"));}
async function field(name,value){await $(`input[name='${name}']`).setValue(value);}
async function switchPolicy(text){await $(`.policy-toggle*=${text}`).$("input").click();}

describe("Predictable repeated and draft input",()=>{
  afterEach(async function(){writeFileSync(join(root,'output',this.currentTest.title.match(/INPUT-\d+/)[0]+'-input-events.json'),JSON.stringify(await browser.execute(()=>({events:window.__MSV_E2E__.nativeInputEvents,active:document.activeElement?.getAttribute('aria-label')})),null,2));});
  before(async()=>{
    mkdirSync(commaPath);mkdirSync(secondPath);
    await browser.execute(async root=>{
      const h=window.__MSV_E2E__;
      await h.client.addProcess({...h.processFixture("native-input","/bin/sleep",["60"]),cwd:root,env:{DATABASE:"base",KEEP:"stay"},liveness:{kind:{type:"http",url:"http://127.0.0.1:8123/custom",expected_status:200},interval_ms:5000,timeout_ms:3000,consecutive_successes:1,consecutive_failures:3}});
      for(const name of ["first","second","이름,쉼표","native-dependencies","native-bulk"]){await h.client.addJob(h.jobFixture(name));}
      await h.client.addJob({...h.jobFixture("native-interval"),trigger:{type:"interval",everySec:120}});
    },root);
  });

  it("INPUT-01 [N-R] comma and Enter commit exact dependencies into successive rows",async()=>{
    await openJob("native-dependencies");await select(".schedule-fields select","depends_on");
    await browser.execute(()=>window.__MSV_E2E__.beginNativeInputObservation());
    await $("input[aria-label='선행 작업 1']").setValue("first");await browser.keys(",");
    writeFileSync(join(root,"output/delimiter-observation.json"),JSON.stringify(await browser.execute(()=>({hasFocus:document.hasFocus(),active:document.activeElement.getAttribute('aria-label'),events:window.__MSV_E2E__.nativeInputEvents,values:[...document.querySelectorAll('.repeated-row input')].map(node=>node.value),suggestions:[...document.querySelectorAll('[role=option]')].map(node=>node.textContent)})),null,2));
    await browser.waitUntil(()=>browser.execute(()=>document.activeElement.getAttribute("aria-label")==="선행 작업 2"));
    await $("input[aria-label='선행 작업 2']").setValue("second");await browser.keys("Enter");
    await browser.waitUntil(()=>browser.execute(()=>document.activeElement.getAttribute("aria-label")==="선행 작업 3"));
    await saveJob();
    const saved=await browser.execute(()=>window.__MSV_E2E__.client.getJobConfig("native-dependencies"));
    assert.deepEqual(saved.trigger.jobs,["first","second"]);
  });

  it("INPUT-02 [N-R] duplicate, self, literal comma, empty delimiter and paste retain intent",async()=>{
    await openJob("native-dependencies");
    await $("input[aria-label='선행 작업 1']").setValue("second");
    assert.equal(await $("dialog .editor-footer button:last-child").isEnabled(),false);
    await $("input[aria-label='선행 작업 1']").setValue("first");
    const third=await $("input[aria-label='선행 작업 3']");await third.setValue("native-dependencies");
    assert.equal(await $("dialog .editor-footer button:last-child").isEnabled(),false);
    await third.setValue("unknown");assert.equal(await $("dialog .editor-footer button:last-child").isEnabled(),false);
    await third.click();await third.setValue("이름");await third.click();const literalName=await $("//button[@role='option' and text()='이름,쉼표']");await literalName.waitForExist();await browser.pause(180);assert.equal(await literalName.isExisting(),true);await literalName.click();
    const count=(await $$(".repeated-row")).length;
    await $("input[aria-label='선행 작업 4']").click();await browser.keys(",");
    assert.equal((await $$(".repeated-row")).length,count);
    await saveJob();
    assert.deepEqual((await browser.execute(()=>window.__MSV_E2E__.client.getJobConfig("native-dependencies"))).trigger.jobs,["first","second","이름,쉼표"]);
    await openJob("native-bulk");await select(".schedule-fields select","depends_on");
    await browser.execute(()=>{const data=new DataTransfer();data.setData("text/plain","first, second");document.querySelector("input[aria-label='선행 작업 1']").dispatchEvent(new ClipboardEvent("paste",{bubbles:true,cancelable:true,clipboardData:data}));});
    await $("button=확인한 항목 추가").click();await saveJob();
    assert.deepEqual((await browser.execute(()=>window.__MSV_E2E__.client.getJobConfig("native-bulk"))).trigger.jobs,["first","second"]);
  });

  it("INPUT-03 [N-R] comma-bearing absolute paths stay literal and invalid exclusions block save",async()=>{
    await openProcess();await $("button=자동 재시작").click();await switchPolicy("파일 변경");
    await $("input[aria-label='감시할 절대 경로 1']").setValue(commaPath);await browser.keys("Enter");
    await $("input[aria-label='감시할 절대 경로 2']").setValue(secondPath);await browser.keys("Enter");
    await $("input[aria-label='제외할 절대 경로 1']").setValue("*.tmp");
    assert.equal(await $("button=설정 적용").isEnabled(),false);
    await $("input[aria-label='제외할 절대 경로 1']").setValue(join(root,"ignored, path"));
    await saveProcess();
    const saved=await config();assert.deepEqual(saved.watch.roots,[commaPath,secondPath]);assert.deepEqual(saved.watch.exclusions,[join(root,"ignored, path")]);
  });

  it("INPUT-04 [N-R] blank numeric drafts remain blank and 5000/3000ms reaches HTTP checks",async function(){
    this.timeout(35000);
    const timings=[];const sockets=new Set();
    const server=createServer((request,response)=>{const entry={at:performance.now(),closed:null};timings.push(entry);response.on("close",()=>{entry.closed=performance.now();});if(timings.length===1){response.writeHead(200);response.end("ok");}});
    server.on("connection",socket=>{sockets.add(socket);socket.on("close",()=>sockets.delete(socket));});
    await new Promise(resolve=>server.listen(0,"127.0.0.1",resolve));
    try {
      await openProcess();await $("button=관리·종료").click();await field("process.instances","");
      assert.equal(await $("input[name='process.instances']").getValue(),"");assert.equal(await $("button=설정 적용").isEnabled(),false);
      await field("process.instances","1");await $("button=자동 재시작").click();await switchPolicy("파일 변경");
      await $("button=상태 확인").click();await field("liveness.interval_ms","");
      assert.equal(await $("input[name='liveness.interval_ms']").getValue(),"");assert.equal(await $("button=설정 적용").isEnabled(),false);
      await field("liveness.interval_ms","5000");await field("liveness.timeout_ms","5000");assert.equal(await $("button=설정 적용").isEnabled(),false);
      await field("liveness.timeout_ms","3000");
      await $(".guard-controls .editor-columns input:not([name])").setValue(`http://127.0.0.1:${server.address().port}/health`);
      await saveProcess();const saved=await config();assert.equal(saved.liveness.interval_ms,5000);assert.equal(saved.liveness.timeout_ms,3000);
      await $("button[aria-label='native-input 시작']").click();
      await browser.waitUntil(()=>timings.length>=2&&timings[1].closed!==null,{timeout:18000});
      const interval=timings[1].at-timings[0].at;const timeout=timings[1].closed-timings[1].at;
      assert.ok(interval>=4500&&interval<8500,`observed interval ${interval}`);assert.ok(timeout>=2500&&timeout<6500,`observed timeout ${timeout}`);
      writeFileSync(join(root,"output/health-timings.json"),JSON.stringify({configuredIntervalMs:5000,configuredTimeoutMs:3000,observedIntervalMs:interval,observedTimeoutMs:timeout},null,2));
    } finally {
      await browser.execute(async()=>{const h=window.__MSV_E2E__;const process=await h.client.getProcess("native-input");if(process.state==="running"||process.state==="starting")await h.client.stopProcess("native-input");});
      for(const socket of sockets)socket.destroy();await new Promise(resolve=>server.close(resolve));
    }
  });

  it("INPUT-05 [N-R] inactive HTTP, TCP and interval drafts restore without serialization",async()=>{
    await openProcess();await $("button=상태 확인").click();
    await $(".guard-controls .editor-columns input:not([name])").setValue("http://127.0.0.1:8999/draft");
    await select(".guard-controls select","tcp");await select(".guard-controls select","http");
    assert.equal(await $(".guard-controls .editor-columns input:not([name])").getValue(),"http://127.0.0.1:8999/draft");
    await switchPolicy("응답 확인");await switchPolicy("응답 확인");
    assert.equal(await $(".guard-controls .editor-columns input:not([name])").getValue(),"http://127.0.0.1:8999/draft");
    await switchPolicy("응답 확인");await saveProcess();assert.equal((await config()).liveness==null,true);
    await openJob("native-interval");await select(".schedule-fields select","cron");await select(".schedule-fields select","interval");
    assert.equal(await $("input[name='trigger.interval.seconds']").getValue(),"120");await saveJob();
  });

  it("INPUT-06 [N-R] environment rows stay visible, validate, undo and import multiline values",async()=>{
    await openProcess();await $("button=환경변수").click();await $("input[aria-label='환경변수 검색']").setValue("DATABASE");
    await $("button=추가").click();let names=await $$("input[aria-label='환경변수 이름']");assert.equal(names.length,2);
    await names[1].setValue("EXTRA");await $("textarea[aria-label='EXTRA 값']").setValue("line1\nline2, literal");
    names=await $$("input[aria-label='환경변수 이름']");await names[1].setValue("bad-key");
    await browser.execute(()=>document.activeElement.blur());assert.equal(await names[1].getValue(),"bad-key");assert.equal(await $("button=설정 적용").isEnabled(),false);
    await names[1].setValue("DATABASE");assert.equal(await $("button=설정 적용").isEnabled(),false);await names[1].setValue("EXTRA");
    await $("button[aria-label='EXTRA 삭제']").click();await $("button=삭제 되돌리기").click();assert.equal(await $("textarea[aria-label='EXTRA 값']").getValue(),"line1\nline2, literal");
    await $("textarea[aria-label='DATABASE 값']").setValue("local");
    await browser.execute(()=>{const data=new DataTransfer();data.items.add(new File(['DATABASE=file\nMULTI="one\\ntwo"\n'],"native.env",{type:"text/plain"}));const input=document.querySelector("input[type='file']");input.files=data.files;input.dispatchEvent(new Event("change",{bubbles:true}));});
    await $(".env-merge-modal").waitForExist();assert.ok((await $(".env-merge-modal").getText()).includes("충돌 1"));
    await $("button[aria-label='DATABASE 파일 값 채택']").click();await $("button=초안에 반영").click();
    assert.equal((await config()).env.DATABASE,"base");await saveProcess();
    const saved=await config();assert.equal(saved.env.DATABASE,"file");assert.equal(saved.env.MULTI,"one\ntwo");assert.equal(saved.env.EXTRA,"line1\nline2, literal");assert.equal(saved.env.KEEP,undefined);
  });

  it("INPUT-08 [N-R] unsupported symlinks and rolling prerequisites are explicit",async()=>{
    await openProcess();await $("button=자동 재시작").click();await switchPolicy("파일 변경");
    const symlink=await $(".editor-check*=심볼릭 링크").$("input");assert.equal(await symlink.isEnabled(),false);
    await switchPolicy("파일 변경");await $("button=관리·종료").click();await switchPolicy("순차 재시작");
    assert.equal(await $("button=설정 적용").isEnabled(),false);await $("button=상태 확인 설정 열기").click();await switchPolicy("준비 상태");
    await $("button=관리·종료").click();await field("rolling.max_unavailable","1");assert.equal(await $("button=설정 적용").isEnabled(),false);await field("rolling.max_unavailable","0");
    await saveProcess();assert.ok((await config()).readiness);
  });
});
