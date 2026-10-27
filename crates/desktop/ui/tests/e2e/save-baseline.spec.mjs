import assert from 'node:assert/strict';
import {writeFileSync} from 'node:fs';
import {join} from 'node:path';

describe('Save lifecycle baseline',()=>{
  it('records lost close draft, stale revision and overwritten late edit',async()=>{
    await browser.execute(async()=>{const h=window.__MSV_E2E__;await h.client.addProcess(h.processFixture('native-save','/bin/sleep',['60']));await h.client.addJob(h.jobFixture('native-save-job'));const d=await h.client.getWorkflow('native-save-job');await h.client.saveWorkflow('native-save-job',h.workflowFixture(),d.revision);});
    await $("button[aria-label='native-save 더 보기']").waitForExist();await $("button[aria-label='native-save 더 보기']").click();await $('.overflow-menu button').click();await $("input[aria-label='전체 실행 명령 인수']").setValue('changed');await browser.keys('Escape');
    const closedDirty=!(await $('dialog').isExisting());
    await $('button=작업').click();await $('.job-table-row*=native-save-job').waitForExist();await $('.job-table-row*=native-save-job').click();await $('button=흐름 편집').click();await $('button=작업 설정').waitForExist();
    const before=await $('.workflow-heading h2').getText();await $('button=작업 설정').click();await $("input[aria-label='전체 실행 명령 인수']").waitForExist();await $("input[aria-label='전체 실행 명령 인수']").setValue('%s settings');await $('dialog .editor-footer button:last-child').click();await $('dialog').waitForExist({reverse:true});
    const after=await $('.workflow-heading h2').getText();const saved=await browser.execute(()=>window.__MSV_E2E__.client.getWorkflow('native-save-job'));
    await $('button=프로세스').click();await $('button=작업').click();await $('.job-table-row*=native-save-job').waitForExist();await $('.job-table-row*=native-save-job').click();await $('button=흐름 편집').click();await $(".react-flow__node[data-id='native-command']").waitForExist();await $(".react-flow__node[data-id='native-command']").click();
    await $('.step-settings .editor-field input').setValue('submitted');
    await browser.execute(()=>{const c=window.__MSV_E2E__.client;window.__originalSave=c.saveWorkflow;c.saveWorkflow=async(...args)=>{const result=await window.__originalSave(...args);window.__saveResponseReady=true;await new Promise(resolve=>window.__releaseSave=resolve);return result;};});
    await $('button=저장').click();await browser.waitUntil(()=>browser.execute(()=>window.__saveResponseReady===true));
    await $('.step-settings .editor-field input').setValue('later');await browser.execute(()=>window.__releaseSave());await browser.waitUntil(async()=>await $('button=저장').isEnabled()===false);
    const lateValue=await $('.step-settings .editor-field input').getValue();await browser.execute(()=>{window.__MSV_E2E__.client.saveWorkflow=window.__originalSave;});
    writeFileSync(join(process.env.MSV_DESKTOP_E2E_ROOT,'output/save-baseline.json'),JSON.stringify({closedDirty,before,after,actualRevision:saved.revision,lateValue},null,2));
    assert.equal(closedDirty,true);assert.equal(before,after);assert.equal(lateValue,'submitted');
  });
});
