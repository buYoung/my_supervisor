import assert from 'node:assert/strict';
import {existsSync, mkdirSync, readFileSync, statSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const root = process.env.MSV_DESKTOP_E2E_ROOT;
const folder = join(root, 'native, 경로');
const rawLog = '{"message":"native 한글, clipboard"}';
const observations = [];

async function observe(phase, details = {}) {
  writeFileSync(join(root, 'output/os-ready.json'), JSON.stringify({phase, ...details}, null, 2));
  let action;
  await browser.waitUntil(() => {
    const file = join(root, 'output/os-actions.json');
    if (!existsSync(file)) return false;
    action = JSON.parse(readFileSync(file, 'utf8')).find(row => row.phase === phase);
    return Boolean(action);
  }, {timeout: 180000, timeoutMsg: `Native observation missing: ${phase}`});
  observations.push(action);
}

async function openProcess(name) {
  await $('button=프로세스').click();
  await $(`button[aria-label='${name} 더 보기']`).waitForExist();
  await $(`button[aria-label='${name} 더 보기']`).click();
  await $('.overflow-menu button').click();
}

describe('Integrated native OS gates', () => {
  before(async () => {
    mkdirSync(folder);
    await browser.execute(async root => {
      const h = window.__MSV_E2E__;
      await h.client.addProcess({...h.processFixture('native-os', '/bin/sleep', ['600']), cwd: root});
      await h.client.addProcess(h.processFixture('native-clipboard', '/usr/bin/printf', ['%s\n', '{"message":"native 한글, clipboard"}']));
    }, root);
  });
  after(() => writeFileSync(join(root, 'output/os-observations.json'), JSON.stringify(observations, null, 2)));
  afterEach(async () => {
    for (let count = 0; count < 8; count++) {
      if(await $('button=변경 버리기').isExisting()) {await $('button=변경 버리기').click();continue;}
      if(!await $('dialog[open]').isExisting()) break;
      const dialogs = await $$('dialog[open]');
      const dialog = dialogs.at(-1);
      if (await dialog.$('button=변경 버리기').isExisting()) await dialog.$('button=변경 버리기').click();
      else await dialog.$('button[aria-label="닫기"]').click();
    }
  });

  it('GATE-07 [O] native folder cancellation and selection preserve exact paths', async () => {
    await openProcess('native-os');
    const field = await $('dialog .path-field-row input');
    assert.equal(await field.getValue(), root);
    await $('button[title="폴더 선택"]').click();
    await observe('picker-cancel', {expected: root});
    assert.equal(await field.getValue(), root);
    assert.equal(await browser.execute(() => window.__MSV_E2E__.client.getProcessConfig('native-os').then(c => c.cwd)), root);
    await $('button[title="폴더 선택"]').click();
    await observe('picker-select', {path: folder});
    const selectedPath = await browser.execute(() => document.querySelector('dialog .path-field-row input').value);
    assert.equal(statSync(selectedPath).ino, statSync(folder).ino, `Unexpected native path: ${selectedPath}`);
    assert.equal(await browser.execute(() => window.__MSV_E2E__.client.getProcessConfig('native-os').then(c => c.cwd)), root);
    await $('button=설정 적용').click();
    await $('dialog').waitForExist({reverse: true});
    assert.equal(await browser.execute(() => window.__MSV_E2E__.client.getProcessConfig('native-os').then(c => c.cwd)), selectedPath);
    observations.push({phase: 'picker-final-consumer', cancelledValue: root, expectedDirectory: folder, selectedAndSaved: selectedPath});
  });

  it('GATE-08 [O] raw clipboard copy is pasted by the native keyboard unchanged', async () => {
    await browser.execute(() => window.__MSV_E2E__.client.startProcess('native-clipboard'));
    await $("button[aria-label='native-clipboard 상세 보기']").click();
    await $('.process-detail').$('button=로그').click();
    await $('.process-log-preview .log-single').waitForExist();
    for(const row of await $$('.process-log-preview .log-single')) {
      if((await row.getText()).includes('native 한글, clipboard')) {await row.click();break;}
    }
    assert.equal(await $('dialog [data-raw-log]').getText(), rawLog);
    await observe('clipboard-copy', {expected: rawLog});
    await browser.waitUntil(async () => (await $('dialog').getText()).includes('원본 로그를 복사했습니다.'));
    await $('dialog button[aria-label="닫기"]').click();
    const search = await $("input[aria-label='프로세스 검색']");
    await search.execute(node => {
      window.__nativePaste = undefined;
      node.addEventListener('paste', event => {window.__nativePaste = {isTrusted: event.isTrusted, text: event.clipboardData.getData('text/plain')};}, {once: true});
      node.focus();
    });
    await observe('clipboard-paste', {expected: rawLog});
    assert.equal(await browser.execute(() => document.querySelector("input[aria-label='프로세스 검색']").value), rawLog);
    const paste = await browser.execute(() => window.__nativePaste);
    assert.deepEqual(paste, {isTrusted: true, text: rawLog});
    observations.push({phase: 'clipboard-final-consumer', paste});
    await search.setValue('');
  });

  it('GATE-10 [O] native executable selection and cancellation preserve argv without execution', async () => {
    await openProcess('native-os');
    await $('.runtime-chip').click();
    await $('button=실행 파일 찾기').click();
    await observe('file-picker-cancel', {expected: '/bin/sleep'});
    assert.equal(await browser.execute(() => document.querySelector('dialog .runtime-location button').textContent), '/bin/sleep');
    await $('button=실행 파일 찾기').click();
    await observe('file-picker-select', {path: '/usr/bin/printf'});
    await browser.waitUntil(() => browser.execute(() => document.querySelector('dialog .runtime-location button').textContent === '/usr/bin/printf'));
    assert.equal(await browser.execute(() => window.__MSV_E2E__.client.getProcessConfig('native-os').then(c => c.command)), '/bin/sleep');
    await $('button=설정 적용').click();
    await $('dialog').waitForExist({reverse: true});
    const saved = await browser.execute(async () => ({config: await window.__MSV_E2E__.client.getProcessConfig('native-os'), status: await window.__MSV_E2E__.client.getProcess('native-os')}));
    assert.equal(saved.config.command, '/usr/bin/printf');
    assert.deepEqual(saved.config.args, ['600']);
    assert.equal(saved.status.state, 'stopped');
    observations.push({phase: 'file-picker-final-consumer', saved});
  });

  it('GATE-03 [O] close to tray keeps owned work alive and Show Window restores state', async () => {
    const work = await browser.execute(async () => {
      const h = window.__MSV_E2E__;
      const current = await h.client.getProcessConfig('native-os');
      if(current.command !== '/bin/sleep') await h.client.updateProcessConfig('native-os', {...current, command:'/bin/sleep', args:['600']}, current);
      await h.client.startProcess('native-os');
      await h.client.addJob(h.jobFixture('native-tray-job'));
      const saved = await h.client.getWorkflow('native-tray-job');
      const flow = h.workflowFixture();
      flow.nodes[1].data = {...flow.nodes[1].data, program: '/bin/sleep', args: ['600']};
      await h.client.saveWorkflow('native-tray-job', flow, saved.revision);
      const run = await h.client.triggerWorkflow('native-tray-job', {single_job: true});
      return {runId: run.run_id, pid: (await h.client.getProcess('native-os')).pid};
    });
    await browser.waitUntil(() => browser.execute(async runId => (await window.__MSV_E2E__.client.getRun('native-tray-job', runId)).state === 'running', work.runId));
    await $("button[aria-label='native-os 상세 보기']").click();
    await observe('tray-close', work);
    assert.equal(await browser.execute(() => window.__MSV_E2E__.isWindowVisible()), false);
    const hidden = await browser.execute(async runId => {
      const h = window.__MSV_E2E__;
      return {process: await h.client.getProcess('native-os'), run: await h.client.getRun('native-tray-job', runId)};
    }, work.runId);
    assert.equal(hidden.process.pid, work.pid);
    assert.equal(hidden.process.state, 'running');
    assert.equal(hidden.run.state, 'running');
    await observe('tray-reopen', work);
    await browser.waitUntil(() => browser.execute(() => window.__MSV_E2E__.isWindowVisible()));
    assert.equal(await $('.process-detail h2').getText(), 'native-os');
    await browser.execute(async runId => {
      const h = window.__MSV_E2E__;
      await h.client.cancelRun('native-tray-job', runId);
    }, work.runId);
    await browser.waitUntil(() => browser.execute(async runId => (await window.__MSV_E2E__.client.getRun('native-tray-job', runId)).state === 'cancelled', work.runId));
    assert.equal(await browser.execute(() => window.__MSV_E2E__.client.getProcess('native-os').then(p => p.state)), 'running');
    await browser.execute(() => window.__MSV_E2E__.client.stopProcess('native-os'));
    observations.push({phase: 'tray-final-consumer', work, hidden, reopenedDetail: 'native-os', explicitCancellation: 'cancelled'});
  });
});
