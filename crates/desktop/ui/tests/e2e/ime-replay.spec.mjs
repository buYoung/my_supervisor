import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {resolve,join} from 'node:path';

it('INPUT-07 [N-R] isolated replay of the recorded Korean composition preserves row boundaries',async()=>{
  const root=process.env.MSV_DESKTOP_E2E_ROOT;
  const source=resolve('../../../docs/evidence/desktop-ux/10-artifacts/os-vm/os-ime-frame/ime-checkpoint.json');
  const recorded=JSON.parse(readFileSync(source,'utf8'));
  assert.ok(recorded.events.some(event=>event.isTrusted&&event.keyCode===229));
  await browser.execute(async()=>{const h=window.__MSV_E2E__;await h.client.addJob(h.jobFixture('한글'));await h.client.addJob(h.jobFixture('ime-replay'));});
  await $('button=작업').click();await $('.job-table-row*=ime-replay').waitForExist();await $('.job-table-row*=ime-replay').click();await $('button=흐름 편집').click();await $('button=작업 설정').waitForExist();await $('button=작업 설정').click();
  await $('.schedule-fields select').waitForExist();
  await browser.execute(()=>{const input=document.querySelector('.schedule-fields select');input.value='depends_on';input.dispatchEvent(new Event('change',{bubbles:true}));});
  await $("input[aria-label='선행 작업 1']").waitForExist();
  const result=await browser.execute(async events=>{
    const snapshots=[];
    document.querySelector("input[aria-label='선행 작업 1']").focus();
    for(const event of events){
      const target=document.querySelector(`input[aria-label='${event.focused||'선행 작업 1'}']`)||document.querySelector("input[aria-label='선행 작업 1']");
      if(event.type==='input'){
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(target,event.value??'');
        target.dispatchEvent(new InputEvent('input',{bubbles:true,data:event.data,inputType:event.inputType,isComposing:event.isComposing}));
      }else if(event.type==='keydown'||event.type==='keyup'){
        target.dispatchEvent(new KeyboardEvent(event.type,{bubbles:true,cancelable:true,key:event.key,keyCode:event.keyCode,isComposing:event.isComposing}));
      }
      // Keep the native replacement and its Enter in one gesture. A key release
      // ends that gesture and lets React commit before the next physical key.
      if(event.type==='keyup')await new Promise(resolve=>requestAnimationFrame(resolve));
      else await Promise.resolve();
      snapshots.push({type:event.type,key:event.key,keyCode:event.keyCode,rows:document.querySelectorAll('.repeated-row').length});
    }
    await new Promise(resolve=>requestAnimationFrame(resolve));
    return {snapshots,values:[...document.querySelectorAll('.repeated-row input')].map(input=>input.value),active:document.activeElement.getAttribute('aria-label'),nativeKeys:window.__MSV_E2E__.nativeKeyEventCount};
  },recorded.events);
  assert.deepEqual(result.values,['한글','']);assert.equal(result.active,'선행 작업 2');assert.equal(result.nativeKeys,0);
  assert.deepEqual(result.snapshots.filter(event=>event.type==='keyup'&&event.key==='Enter').map(event=>event.rows),[1,2]);
  assert.ok(result.snapshots.filter(event=>event.keyCode===229).every(event=>event.rows===1));
  assert.equal((await browser.execute(()=>window.__MSV_E2E__.client.getJobConfig('ime-replay'))).trigger.type,'one_shot');
  writeFileSync(join(root,'output/ime-replay.json'),JSON.stringify({source,mode:'recorded event replay, not a new OS observation',result},null,2));
});
