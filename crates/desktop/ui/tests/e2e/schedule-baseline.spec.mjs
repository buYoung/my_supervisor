import assert from 'node:assert/strict';
import {writeFileSync} from 'node:fs';
import {join} from 'node:path';
describe('Schedule baseline',()=>{
 it('records forced UTC, invented timeout and stale preview',async()=>{
  const direct=await browser.execute(async()=>{const h=window.__MSV_E2E__;await h.client.addJob(h.jobFixture('native-server-default'));return h.client.getJobConfig('native-server-default');});
  await $('button=작업').click();await $('button=작업 추가').click();await $('dialog .editor-field input').setValue('native-schedule-ui');await browser.execute(()=>{const select=document.querySelector('.schedule-fields select');select.value='cron';select.dispatchEvent(new Event('change',{bubbles:true}));});
  await $('.schedule-fields .editor-field input').setValue('0 3 * * *');
  const shownTimeout=await $("input[name='job.timeout']").getValue();
  await $('button=다음 실행 시각 확인').click();await $('.schedule-preview').waitForExist();const oldDates=await $$('.schedule-preview').map(row=>row.getText());await $('.schedule-fields .editor-field input').setValue('0 4 * * *');const newDates=await $$('.schedule-preview').map(row=>row.getText());await $('dialog .editor-footer button:last-child').click();await $('dialog').waitForExist({reverse:true});
  const gui=await browser.execute(()=>window.__MSV_E2E__.client.getJobConfig('native-schedule-ui'));
  writeFileSync(join(process.env.MSV_DESKTOP_E2E_ROOT,'output/schedule-baseline.json'),JSON.stringify({serverTimezone:direct.timezone,guiTimezone:gui.timezone,shownTimeout,savedTimeout:gui.timeout_sec??null,oldDates,newDates},null,2));
  assert.equal(gui.timezone,'UTC');assert.equal(shownTimeout,'3600');assert.equal(gui.timeout_sec,undefined);assert.deepEqual(oldDates,newDates);
 });
});
