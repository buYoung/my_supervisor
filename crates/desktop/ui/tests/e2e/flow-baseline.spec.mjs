import assert from 'node:assert/strict';
import {writeFileSync} from 'node:fs';
import {join} from 'node:path';
describe('Workflow baseline',()=>{it('captures the current edit controls and viewport reset',async()=>{
 await browser.execute(async()=>{const h=window.__MSV_E2E__;await h.client.addJob(h.jobFixture('flow-baseline'));const d=await h.client.getWorkflow('flow-baseline');await h.client.saveWorkflow('flow-baseline',h.workflowFixture(),d.revision);});
 await $('button=작업').click();await $('.job-table-row*=flow-baseline').waitForExist();await $('.job-table-row*=flow-baseline').click();await $('button=흐름 편집').click();await $(".react-flow__node[data-id='native-command']").waitForExist();
 const before=await $('.react-flow__viewport').getAttribute('style');await $('button=단계 추가').click();await $('.step-palette').waitForExist();const after=await $('.react-flow__viewport').getAttribute('style');
 const value={undo:await $('button=실행 취소').isExisting(),before,after,hasDraftScopeLabel:(await $('.workflow-heading').getText()).includes('초안')};assert.equal(value.undo,false);writeFileSync(join(process.env.MSV_DESKTOP_E2E_ROOT,'output/flow-baseline.json'),JSON.stringify(value,null,2));
});});
