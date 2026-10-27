import assert from "node:assert/strict";
import {writeFileSync, readFileSync, existsSync, rmSync} from "node:fs";
import {execFileSync} from "node:child_process";
import {join} from "node:path";

const root=process.env.MSV_DESKTOP_E2E_ROOT;
const output=join(root,"output/argv.json");
const helpMarker=join(root,"output/help-executed");
const recorder=join(root,"native-recorder");
const slow=join(root,"slow-help");
const slowPid=join(root,"output/slow.pid");
const events=[];

async function openProcess(name) {
  await $("button=프로세스").click();
  await $(`button[aria-label='${name} 더 보기']`).waitForExist();
  await $(`button[aria-label='${name} 더 보기']`).click();
  await $(".overflow-menu button").click();
  await $("input[aria-label='전체 실행 명령 인수']").waitForExist();
}
async function cancelEditor(){await $("button=취소").click();if(await $("button=변경 버리기").isExisting())await $("button=변경 버리기").click();}
async function setArgs(line) {await $("input[aria-label='전체 실행 명령 인수']").setValue(line);}
async function saveProcess() {
  await $("button=설정 적용").click();
  await $("dialog").waitForExist({reverse:true});
}
async function stored(name="native-command") {
  return browser.execute(name=>window.__MSV_E2E__.client.getProcessConfig(name),name);
}
async function executeRecorder(expected) {
  rmSync(output,{force:true});
  await $("button[aria-label='native-command 시작']").waitForExist();
  await $("button[aria-label='native-command 시작']").click();
  await browser.waitUntil(()=>existsSync(output));
  const actual=JSON.parse(readFileSync(output,"utf8"));
  assert.deepEqual(actual.args,expected);
  assert.equal(actual.env,"한글, value");
  events.push({expected,actual});
  await browser.waitUntil(async()=>!await $("button[aria-label='native-command 중지']").isExisting());
}

describe("Safe command editing",()=>{
  before(async()=>{
    writeFileSync(recorder,`#!${process.execPath}\nconst fs=require('node:fs');\nif(process.argv.includes('--help')) {fs.appendFileSync(${JSON.stringify(helpMarker)},'probe\\n'); console.log('Usage: recorder\\n  --port <port>  Port number');}\nelse fs.writeFileSync(${JSON.stringify(output)},JSON.stringify({args:process.argv.slice(2),env:process.env.COMMAND_TEST}));\n`,{mode:0o700});
    writeFileSync(slow,`#!${process.execPath}\nrequire('node:fs').writeFileSync(${JSON.stringify(slowPid)},String(process.pid));\nsetTimeout(()=>console.log('Usage: slow\\n  --old <value>  Old value'),10000);\n`,{mode:0o700});
    await browser.execute(async program=>{
      const h=window.__MSV_E2E__;
      await h.client.addProcess({...h.processFixture("native-command",program,["run","start","--","--port","3000"]),env:{COMMAND_TEST:"한글, value"}});
    },recorder);
  });

  it("CMD-01 [N-R] ordered port edit reaches the real child argv and environment",async()=>{
    await openProcess("native-command");
    const values=await $$(".argument-row input[aria-label='옵션 값']");
    await values[values.length-1].setValue("4000");
    await saveProcess();
    const config=await stored();
    assert.deepEqual(config.args,["run","start","--","--port","4000"]);
    await executeRecorder(config.args);
  });

  it("CMD-02 [N-R] empty, literal, repeated and executable-name arguments round trip",async()=>{
    const cases=[
      {line:"'' '값,alpha' 'a b' '한글'",args:["","값,alpha","a b","한글"]},
      {line:"native-recorder --tag=a --tag=b --empty= 'quote'\\''value'",args:["native-recorder","--tag=a","--tag=b","--empty=","quote'value"]},
    ];
    for(const test of cases){
      await openProcess("native-command");await setArgs(test.line);await saveProcess();
      assert.deepEqual((await stored()).args,test.args);
      await executeRecorder(test.args);
    }
    writeFileSync(join(root,"output/command-final-consumer.json"),JSON.stringify(events,null,2));
  });

  it("CMD-03 [N-M] invalid drafts never submit and survive editor tabs",async()=>{
    await openProcess("native-command");await setArgs("'unclosed");
    await browser.execute(()=>window.__MSV_E2E__.enableMockRouting());
    const mock=await browser.tauri.mock("cmd_update_process_config");
    await mock.mockReturnValue(await browser.execute(()=>window.__MSV_E2E__.processStatusFixture("native-command")));
    assert.equal(await $("button=설정 적용").isEnabled(),false);
    await $("button=환경변수").click();await $("button=실행").click();
    assert.equal(await $("input[aria-label='전체 실행 명령 인수']").getValue(),"'unclosed");
    assert.equal(await $("button=설정 적용").isEnabled(),false);
    assert.equal(mock.mock.calls.length,0);
    await browser.tauri.restoreAllMocks();await browser.execute(()=>window.__MSV_E2E__.restoreInvoke());
    await setArgs("corrected");await saveProcess();
    assert.deepEqual((await stored()).args,["corrected"]);
  });

  it("CMD-04 [N-R] editing is inert and help is an explicit execution",async()=>{
    rmSync(helpMarker,{force:true});
    await openProcess("native-command");await setArgs("typed without executing");
    await $("button=환경변수").click();await $("button=실행").click();
    assert.equal(existsSync(helpMarker),false);
    await $("button=도움말 실행").click();
    await browser.waitUntil(()=>existsSync(helpMarker));
    await browser.waitUntil(async()=>await $("button=도움말 실행").isEnabled());
    assert.equal(await $("input[aria-label='전체 실행 명령 인수']").getValue(),"typed without executing");
    await $("button=인수 추가").click();
    const names=await $$(".argument-row input[aria-label='옵션 이름']");
    await names[0].click();
    await names[names.length-1].click();
    await browser.pause(200);
    assert.ok(await $('.option-completions').isExisting());
    await browser.keys(["ArrowDown","Enter"]);
    assert.equal(await names[names.length-1].getValue(),"--port");
    await cancelEditor();
  });

  it("CMD-05 [N-R] cancelled slow help is bounded and does not replace newer input",async()=>{
    await browser.execute(async program=>{const h=window.__MSV_E2E__;await h.client.addProcess(h.processFixture("native-slow-help",program,[]));},slow);
    await openProcess("native-slow-help");await $("button=도움말 실행").click();
    await browser.waitUntil(()=>existsSync(slowPid));
    await $("button=결과 받기 취소").click();await setArgs("newer draft");
    await browser.waitUntil(async()=>await $("button=도움말 실행").isEnabled(),{timeout:15000});
    assert.equal(await $("input[aria-label='전체 실행 명령 인수']").getValue(),"newer draft");
    const pid=Number(readFileSync(slowPid,"utf8"));
    let state="";
    try{state=execFileSync("ps",["-p",String(pid),"-o","stat="],{encoding:"utf8"});}catch(error){if(error.status!==1)throw error;}
    assert.ok(!state.trim()||state.trim().startsWith("Z"),`slow help remains alive: ${pid}`);
    writeFileSync(join(root,"output/help-cleanup.json"),JSON.stringify({pid,state:state.trim(),newerDraftPreserved:true}));
    await cancelEditor();
  });

  it("CMD-06 [N-R] job, workflow and exec health consumers reject invalid drafts",async()=>{
    await browser.execute(async program=>{
      const h=window.__MSV_E2E__;
      let stage="add command job";
      try {
      await h.client.addJob({...h.jobFixture("native-job-command"),command:program,args:["original"]});
      stage="add flow job";
      await h.client.addJob(h.jobFixture("native-flow-command"));
      stage="get default flow";
      const saved=await h.client.getWorkflow("native-flow-command");
      stage="save fixture flow";
      await h.client.saveWorkflow("native-flow-command",h.workflowFixture(),saved.revision);
      } catch(error) {throw new Error(JSON.stringify({stage,message:error.message,details:error.details}));}
    },recorder);
    await $("button=작업").click();
    await $(".job-table-row*=native-job-command").waitForExist();
    await $(".job-table-row*=native-job-command").click();
    await $("button=흐름 편집").click();
    await $("button=작업 설정").waitForExist();
    await $("button=작업 설정").click();
    await setArgs("'unclosed");
    assert.equal(await $("dialog .editor-footer button:last-child").isEnabled(),false);
    await setArgs("'job,value' ''");
    await $("dialog .editor-footer button:last-child").click();
    await $("dialog").waitForExist({reverse:true});
    const job=await browser.execute(()=>window.__MSV_E2E__.client.getJobConfig("native-job-command"));
    assert.deepEqual(job.args,["job,value",""]);
    await $("button=작업 목록").click();
    await $(".job-table-row*=native-flow-command").click();
    await $("button=흐름 편집").click();
    await $(".react-flow__node[data-id='native-command']").waitForExist();
    await $(".react-flow__node[data-id='native-command']").click();
    await setArgs("'unclosed");
    assert.equal(await $("button=저장").isEnabled(),false);
    assert.equal(await $("button=현재 초안 실행").isEnabled(),false);
    assert.equal(await $("button=이 단계만 실행").isEnabled(),false);
    await $(".react-flow__node[data-id='native-trigger']").click();
    assert.equal(await $("button=현재 초안 실행").isEnabled(),false);
    await $(".react-flow__node[data-id='native-command']").click();
    assert.equal(await $("input[aria-label='전체 실행 명령 인수']").getValue(),"'unclosed");
    await setArgs("'flow,value' ''");
    await $("button=저장").click();
    await browser.waitUntil(async()=>{
      const saved=await browser.execute(()=>window.__MSV_E2E__.client.getWorkflow("native-flow-command"));
      return JSON.stringify(saved.definition.nodes.find(node=>node.id==="native-command").data.args)===JSON.stringify(["flow,value",""]);
    });
    await browser.waitUntil(async()=>await $("button=현재 초안 실행").isEnabled());
    await browser.execute(async program=>{
      const client=window.__MSV_E2E__.client;
      const before=await client.getProcessConfig("native-command");
      await client.updateProcessConfig("native-command",{...before,liveness:{kind:{type:"exec",command:program,args:["health"]},interval_ms:5000,timeout_ms:3000,consecutive_successes:1,consecutive_failures:3}},before);
    },recorder);
    await openProcess("native-command");
    await $("button=상태 확인").click();
    await setArgs("'unclosed");
    assert.equal(await $("button=설정 적용").isEnabled(),false);
    await setArgs("'health,value' ''");
    await saveProcess();
    assert.deepEqual((await stored()).liveness.kind.args,["health,value",""]);
  });
});
