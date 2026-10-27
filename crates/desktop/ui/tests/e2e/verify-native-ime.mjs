import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {pathToFileURL} from 'node:url';

export function verifyNativeIme(observation) {
  const {events,result,actions}=observation;
  const hasCompositionEvents=events.some(event=>event.type==='compositionstart'&&event.isTrusted);
  const hasNativeReplacement=events.some(event=>event.type==='keydown'&&event.keyCode===229&&event.isTrusted)
    && events.some(event=>event.inputType==='insertReplacementText'&&event.isTrusted&&event.rowCount===1);
  assert.ok(hasCompositionEvents||hasNativeReplacement,'No native composition signal');
  if(hasCompositionEvents){
    const end=events.find(event=>event.type==='compositionend'&&event.isTrusted&&event.value==='한글');
    assert.ok(end,'Native Korean compositionend missing');
    assert.equal(end.rowCount,1);assert.equal(end.focused,'선행 작업 1');
  }else if(actions.method==='vm-hardware-keyboard'){
    assert.ok(actions.vmModel?.startsWith('VirtualMac'),'OS keyboard observation must run inside a VM');
    assert.deepEqual(actions.firstEnter,{rowCount:1,values:['한글'],active:'선행 작업 1'},'First OS Return must only commit Korean composition');
  }else{
    assert.equal(actions.answer,'완료: 첫 Enter는 조합 확정, 두 번째는 다음 행','Physical keyboard observation must confirm both Enter actions');
  }
  assert.equal(result.rowCount,2);assert.equal(result.values[0],'한글');assert.equal(result.active,'선행 작업 2');
  return {passed:true,mode:'O',nativeSignal:hasCompositionEvents?'composition-events':'native-229-replacement',method:actions.method??'human-keyboard',humanObservation:actions.answer??null};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
  const result=verifyNativeIme(JSON.parse(readFileSync(process.argv[2],'utf8')));
  const report={...result,observation:process.argv[2],command:process.argv.join(' '),cwd:process.cwd()};
  if(process.argv[3])writeFileSync(process.argv[3],JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify(report));
}
