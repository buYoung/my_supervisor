import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';

it('ISOLATION-01 [N-R] invisible automation stays non-key and keeps rendering without desktop input',async()=>{
  const root=process.env.MSV_DESKTOP_E2E_ROOT;
  const native=JSON.parse(readFileSync(join(root,'output/input-isolation.json'),'utf8'));
  assert.equal(native.applicationActive,false);
  assert.equal(native.focused,false);
  assert.equal(native.opacity,0);
  assert.equal(native.ignoresMouseEvents,true);
  const frames=await browser.execute(async()=>{
    await new Promise((resolve,reject)=>{const timeout=setTimeout(()=>reject(new Error('Animation frame suspended')),3000);requestAnimationFrame(()=>{clearTimeout(timeout);resolve();});});
    return {frames:true,clipboard:await navigator.clipboard.readText()};
  });
  assert.equal(frames.frames,true);
  assert.equal(frames.clipboard,'');
  await $('button=프로세스 추가').click();
  await $('dialog').waitForExist();
  const state=await browser.execute(async()=>({focused:await window.__TAURI__.window.getCurrentWindow().isFocused(),keys:window.__MSV_E2E__.nativeKeyEventCount,dialog:Boolean(document.querySelector('dialog[open]'))}));
  assert.equal(state.focused,false);
  assert.equal(state.keys,0);
  assert.equal(state.dialog,true);
  await $('dialog').$('button[aria-label="닫기"]').click();
  writeFileSync(join(root,'output/isolation-proof.json'),JSON.stringify({native,frames,state},null,2));
});
