import assert from 'node:assert/strict';
import {writeFileSync} from 'node:fs';
import {join} from 'node:path';
describe('Log navigation baseline',()=>{it('records whole-log entry restrictions',async()=>{
 const root=process.env.MSV_DESKTOP_E2E_ROOT;writeFileSync(join(root,'logs/process-log-baseline.jsonl'),JSON.stringify({sequence:1,timestamp:new Date(Date.now()-1000).toISOString(),stream:'stdout',line:'{"level":"info","message":"visible INFO"}'})+'\n');
 await browser.execute(async()=>{const h=window.__MSV_E2E__;await h.client.addProcess(h.processFixture('log-baseline'));const original=h.client.queryLogs;h.client.queryLogs=async query=>{window.__logBaseline=query;return original(query);};});
 await $("button[aria-label='log-baseline 상세 보기']").waitForExist();await $("button[aria-label='log-baseline 상세 보기']").click();await $('.process-detail a').click();await $('h1=로그').waitForExist();await browser.waitUntil(()=>browser.execute(()=>Boolean(window.__logBaseline)));const query=await browser.execute(()=>window.__logBaseline);assert.equal(query.level,'attention');writeFileSync(join(root,'output/log-baseline.json'),JSON.stringify({query,body:await $('body').getText()},null,2));
});});
