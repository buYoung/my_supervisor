import {initialWorkflow} from './workflow-model.js';
import {resultRoutes,selectResultRoute} from './result-routing.js';
import {acquireCircuit,settleCircuit,retryDelaySeconds,circuitState} from './execution-policy.js';
const runs = new Map();
const history = new Map();
const listeners = new Map();
const controllers = new Map();
const storageKey = jobId => `msv-mock-workflow-run-${jobId}`;
const clone = value => structuredClone(value);

export const executionLabels = {pending:'대기',running:'실행 중',succeeded:'완료',failed:'실패',skipped:'건너뜀',cancelled:'취소'};
export const durationLabel = milliseconds => milliseconds == null ? '—' : milliseconds < 1000 ? `${Math.max(0,Math.round(milliseconds))}ms` : `${(milliseconds / 1000).toFixed(1)}초`;

export function executionDefinition(workflow) {
  return JSON.stringify({nodes:workflow.nodes.map(({id,data:{groupName,...data}})=>({id,data})),edges:workflow.edges.map(({source,target,sourceHandle})=>({source,target,sourceHandle}))});
}

export function getWorkflowRuns(jobId) {
  if(!history.has(jobId)) {
    let saved=[];
    try { saved=JSON.parse(sessionStorage.getItem(`msv-mock-workflow-history-${jobId}`))||[]; if(!saved.length){const old=JSON.parse(sessionStorage.getItem(storageKey(jobId)));if(old)saved=[old];} } catch {}
    saved=saved.map((run,index)=>({...run,sequence:run.sequence||saved.length-index,versionLabel:run.versionLabel||'버전 미기록'}));
    for(const run of saved)if(run.state==='running') {
      run.state='cancelled';run.activeNodeId=null;run.finishedAt=new Date().toISOString();
      run.durationMs=Date.parse(run.finishedAt)-Date.parse(run.startedAt);
      for(const result of Object.values(run.steps)) {
        if(result.state==='running'){result.state='cancelled';result.summary='화면을 새로 열어 미리 실행이 종료되었습니다.';}
        if(result.state==='pending'){result.state='skipped';result.summary='실행이 중단되어 실행하지 않았습니다.';}
      }
    }
    history.set(jobId,saved);if(saved[0])runs.set(jobId,saved[0]);
  }
  return clone(history.get(jobId));
}
export function getWorkflowRun(jobId,runId=null) {
  const records=getWorkflowRuns(jobId);
  return runId?records.find(run=>run.id===runId)||null:records[0]||null;
}

export function subscribeWorkflowRun(jobId, callback) {
  if (!listeners.has(jobId)) listeners.set(jobId, new Set());
  listeners.get(jobId).add(callback);
  return () => listeners.get(jobId)?.delete(callback);
}

function publish(run) {
  run.activeNodeIds=Object.values(run.steps).filter(step=>step.state==='running').map(step=>step.id);
  const value=clone(run),records=getWorkflowRuns(run.jobId);
  const index=records.findIndex(record=>record.id===run.id);
  if(index<0)records.unshift(value);else records[index]=value;
  const next=records.slice(0,30);history.set(run.jobId,next);runs.set(run.jobId,next[0]);
  try { sessionStorage.setItem(`msv-mock-workflow-history-${run.jobId}`,JSON.stringify(next));sessionStorage.setItem(storageKey(run.jobId),JSON.stringify(next[0])); } catch {}
  for(const listener of listeners.get(run.jobId)||[])listener(value);
}

function delay(milliseconds, signal) {
  return new Promise((resolve, reject) => {
    const cancelled = () => { clearTimeout(timer); reject(new DOMException('실행 취소', 'AbortError')); };
    const timer = setTimeout(() => { signal.removeEventListener('abort', cancelled); resolve(); }, milliseconds);
    signal.addEventListener('abort', cancelled, {once:true});
    if (signal.aborted) cancelled();
  });
}

function appendLog(run, result, stream, message) {
  result.logs.push({time:new Date().toISOString(),stream,message});
  publish(run);
}

async function executeStep(run, node, context, signal) {
  const {data} = node;
  const result = run.steps[node.id];
  const log = (stream, message) => appendLog(run, result, stream, message);
  log('system', `단계 시작: ${data.label}`);
  const sample=run.resultSamples?.[node.id];
  if(sample) {
      await delay(800,signal);
      const exitCode=sample.outcome==='timeout'?124:Number(sample.exitCode),outcome=sample.outcome==='timeout'?'timeout':exitCode===0?'succeeded':'failed';
      if(sample.stdout)log('stdout',sample.stdout);
      if(sample.stderr)log('stderr',sample.stderr);
      log('system',`입력한 샘플 결과 · 종료 코드 ${exitCode}${outcome==='timeout'?' · 시간 초과':''}`);
      context.exitCode=exitCode;
      return {state:outcome==='succeeded'?'succeeded':'failed',outcome,exitCode,stdout:sample.stdout||'',stderr:sample.stderr||'',summary:outcome==='timeout'?'단계 제한 시간 초과':`단계가 종료 코드 ${exitCode}로 끝났습니다.`};
    }
  if (data.kind === 'trigger') {
    log('system', '수동 실행 요청 수신 · 이 작업의 시작 조건을 건너뜁니다.');
    await delay(350, signal);
    return {state:'succeeded',summary:'수동 실행을 시작했습니다.'};
  }
  if (data.kind === 'command') {
    log('system', `작업 디렉터리: ${data.cwd || '작업 기본 경로'}`);
    log('system', `$ ${data.command}`);
    await delay(800, signal);
    if (run.scenario === 'command_failure'||run.scenario==='transient_failure'&&result.attempt===1) {
      const stderr='검증 실패: 대상 파일을 읽을 수 없습니다. (목업 예시)';log('stderr',stderr);
      context.exitCode = 1;
      return {state:'failed',outcome:'failed',exitCode:1,stdout:'',stderr,summary:'명령이 종료 코드 1로 끝났습니다.'};
    }
    const stdout='작업 처리가 완료되었습니다.';log('stdout',stdout);
    context.exitCode = 0;
    return {state:'succeeded',outcome:'succeeded',exitCode:0,stdout,stderr:'',summary:'명령이 정상 종료되었습니다.'};
  }
  if (data.kind === 'condition') {
    await delay(650, signal);
    const actual = data.source === 'exit_code' ? context.exitCode : true;
    const matched = data.source === 'exit_code' ? data.operator === 'not_equals' ? actual !== Number(data.value) : data.operator === 'gte' ? actual >= Number(data.value) : actual === Number(data.value) : actual;
    const branch = matched ? 'true' : 'false';
    log('system', data.source === 'exit_code' ? `종료 코드 ${actual} ${data.operator === 'equals' ? '=' : data.operator === 'not_equals' ? '≠' : '≥'} ${data.value} → ${matched ? '참' : '거짓'}` : `파일 존재 확인: ${data.path} → 참 (목업 예시)`);
    log('system', `${matched ? '참' : '거짓'} 경로를 선택했습니다.`);
    return {state:'succeeded',branch,summary:`조건 결과: ${matched ? '참' : '거짓'}`,inputValue:String(actual)};
  }
  if (data.kind === 'process') {
    const operation = {start:'시작',stop:'중지',restart:'재시작'}[data.operation];
    log('system', `${data.processName} 프로세스 ${operation} 요청`);
    await delay(1100, signal);
    if (run.scenario === 'process_failure') {
      log('stderr', '프로세스 제어 실패: 응답 대기 시간 초과 (목업 예시)');
      context.exitCode = 1;
      return {state:'failed',exitCode:1,summary:`${data.processName} ${operation}에 실패했습니다.`};
    }
    log('stdout', `${data.processName} ${operation} 완료 · 상태: ${data.operation === 'stop' ? '중지됨' : '실행 중'} (목업 예시)`);
    context.exitCode = 0;
    return {state:'succeeded',exitCode:0,summary:`${data.processName} ${operation} 완료`};
  }
  if(data.kind==='health') {
    const target=data.checkType==='tcp'?`${data.host}:${data.port}`:data.checkType==='process'?data.processName:data.url;
    log('system',`준비 상태 확인: ${target}`);
    await delay(350,signal);log('system','응답 대기 · 첫 번째 확인');await delay(500,signal);
    if(run.scenario==='health_failure'){log('stderr','준비 상태 확인 제한 시간을 초과했습니다. (목업 예시)');context.exitCode=1;return {state:'failed',exitCode:1,summary:'서비스가 준비되지 않았습니다.'};}
    log('stdout',data.checkType==='http'?`HTTP ${data.expectedStatus||200} 응답 확인 (목업 예시)`:'준비 상태 확인 완료 (목업 예시)');
    context.exitCode=0;return {state:'succeeded',exitCode:0,summary:'서비스가 준비되었습니다.'};
  }
  if (data.kind === 'wait') {
    log('system', `${data.seconds}초 대기 · 목업에서는 대기 시간을 축약합니다.`);
    await delay(900, signal);
    return {state:'succeeded',summary:`${data.seconds}초 대기 단계를 완료했습니다.`};
  }
  if (data.kind === 'job') {
    const target = window.SupervisorJobWorkspace.snapshot().jobs.find(job => (job.flowId || job.name) === data.jobId);
    if (!target) { log('stderr','호출할 작업을 찾을 수 없습니다.'); return {state:'failed',exitCode:1,summary:'호출 대상이 없습니다.'}; }
    log('system', `${target.name} 작업 호출 (목업 예시)`);
    await delay(900, signal);
    log('stdout', `${target.name} 완료 응답 수신 (목업 예시)`);
    return {state:'succeeded',exitCode:0,summary:`${target.name} 호출 완료`};
  }
  if(data.kind==='parallel'){log('system',`${data.branches.length}개 분기를 동시에 시작합니다.`);return {state:'succeeded',exitCode:0,summary:'병렬 분기를 시작했습니다.'};}
  if(data.kind==='join'){
    const outcomes=context.parallelResults||[],winner=outcomes.find(item=>item.isWinner);
    const failed=outcomes.filter(item=>item.state!=='succeeded').length;
    const succeeded=data.joinMode==='race'?winner?.state==='succeeded':data.joinMode==='first_success'?outcomes.some(item=>item.state==='succeeded'):data.failurePolicy==='collect'||failed===0;
    log('system',`합류 결과: 성공 ${outcomes.length-failed}개 · 실패/취소 ${failed}개`);
    if(winner)log('system',`${data.joinMode==='race'?'Race 첫 완료':'첫 성공'} 채택: ${winner.label} · ${executionLabels[winner.state]} · 나머지 분기 취소`);
    context.exitCode=data.joinMode==='race'?(winner?.exitCode??(succeeded?0:1)):(succeeded?0:1);
    return {state:succeeded?'succeeded':'failed',exitCode:context.exitCode,branchResults:outcomes,summary:data.joinMode==='race'?`${winner?.label||'첫 분기'}의 ${succeeded?'성공':'실패'} 결과를 채택했습니다.`:succeeded?'병렬 결과를 모았습니다.':'병렬 분기에서 실패했습니다.'};
  }
  await delay(350, signal);
  log(data.result === 'failed' ? 'stderr' : 'system', data.message || (data.result === 'failed' ? '실패 경로로 흐름을 종료합니다.' : '모든 실행 단계를 완료했습니다.'));
  return {state:data.result || 'succeeded',exitCode:data.result === 'failed' ? 1 : 0,summary:data.message || (data.result === 'failed' ? '흐름 실패' : '흐름 완료')};
}

async function executeWithPolicy(run,node,context,signal) {
  const result=run.steps[node.id],attempts=Math.max(1,Math.min(10,Number(node.data.maxAttempts)||1));
  const gate=acquireCircuit(run.jobId,node.id,node.data.circuitBreaker);
  if(!gate.allowed){context.exitCode=null;appendLog(run,result,'system',`서킷 차단 · ${gate.remainingSeconds}초 후 복구 확인`);return {state:'failed',outcome:'circuit_open',exitCode:null,summary:'서킷 브레이커가 새 호출을 차단했습니다.'};}
  if(gate.phase==='half_open')appendLog(run,result,'system','서킷 복구 확인 · 제한된 시험 실행');
  try {for(let attempt=1;attempt<=attempts;attempt++) {
    result.attempt=attempt;result.maxAttempts=attempts;
    if(attempts>1)appendLog(run,result,'system',`시도 ${attempt}/${attempts}`);
    const child=new AbortController();let timedOut=false;
    const cancelled=()=>child.abort();signal.addEventListener('abort',cancelled,{once:true});if(signal.aborted)child.abort();
    const timeout=Number(node.data.timeoutSeconds)>0?setTimeout(()=>{timedOut=true;child.abort();},Number(node.data.timeoutSeconds)*1000):null;
    let output;
    const logStart=result.logs.length;
    try { output=await executeStep(run,node,context,child.signal); }
    catch(error){if(!timedOut||signal.aborted)throw error;appendLog(run,result,'stderr','단계 제한 시간을 초과했습니다.');context.exitCode=124;output={state:'failed',outcome:'timeout',exitCode:124,stdout:'',stderr:'단계 제한 시간을 초과했습니다.',summary:'단계 제한 시간 초과'};}
    finally {clearTimeout(timeout);signal.removeEventListener('abort',cancelled);}
    output={stdout:result.logs.slice(logStart).filter(log=>log.stream==='stdout').map(log=>log.message).join('\n'),stderr:result.logs.slice(logStart).filter(log=>log.stream==='stderr').map(log=>log.message).join('\n'),outcome:output.state==='failed'?'failed':'succeeded',...output};
    if(output.state!=='failed'||attempt===attempts||node.data.retryOn==='timeout'&&output.outcome!=='timeout'){settleCircuit(run.jobId,node.id,node.data.circuitBreaker,output.state);if(node.data.circuitBreaker?.enabled){const circuit=circuitState(run.jobId,node.id);if(circuit.phase==='open')appendLog(run,result,'system',`연속 실패 임계값 도달 · ${node.data.circuitBreaker.openSeconds}초 동안 새 호출 차단`);else if(gate.phase==='half_open')appendLog(run,result,'system',`복구 확인 결과: ${circuit.phase==='closed'?'정상 통과로 전환':'추가 성공 확인 대기'}`);}return output;}
    const seconds=retryDelaySeconds(node.data,attempt,true);
    appendLog(run,result,'system',`${seconds}초 후 이 단계만 재시도합니다. (목업 대기 시간 축약)`);
    await delay(Math.min(seconds*1000,800),signal);
  }}catch(error){settleCircuit(run.jobId,node.id,node.data.circuitBreaker,'cancelled');throw error;}
}

function closeRun(run, state) {
  run.state = state;
  run.activeNodeId = null;
  run.finishedAt = new Date().toISOString();
  run.durationMs = Date.parse(run.finishedAt) - Date.parse(run.startedAt);
  const blocked = new Set(), pending = run.failedNodeId ? [run.failedNodeId] : [];
  while (pending.length) { const id = pending.pop(); if (blocked.has(id)) continue; blocked.add(id); pending.push(...run.workflow.edges.filter(edge=>edge.source===id).map(edge=>edge.target)); }
  for (const result of Object.values(run.steps)) {
    if (result.state !== 'pending') continue;
    result.state = 'skipped';
    result.summary = run.scope === 'step' ? '단독 실행 범위에 포함되지 않습니다.' : state === 'cancelled' ? '실행 취소로 진행하지 않았습니다.' : state === 'failed' && blocked.has(result.id) ? '앞선 단계 실패로 실행하지 않았습니다.' : '선택되지 않은 분기입니다.';
  }
  publish(run);
  window.SupervisorJobWorkspace.finishWorkflowRun?.(run);
}

async function executeParallel(run,node,context,signal,seen) {
  const join=run.workflow.nodes.find(item=>item.id===node.data.joinId);
  if(!join)throw new Error('병렬 합류 단계를 찾을 수 없습니다.');
  const branches=node.data.branches||[],children=branches.map(()=>new AbortController());
  const cancel=()=>children.forEach(child=>child.abort());signal.addEventListener('abort',cancel,{once:true});if(signal.aborted)cancel();
  let hasWinner=false;
  try {
    return await Promise.all(branches.map(async (branch,index)=>{
      const edge=run.workflow.edges.find(item=>item.source===node.id&&item.sourceHandle===branch.id);
      if(!edge)return {id:branch.id,label:branch.label,state:'failed',summary:'연결되지 않은 분기'};
      run.visitedEdges.push(edge.id);publish(run);
      const output=await executePath(run,edge.target,clone(context),children[index].signal,{stopAt:join.id,seen:new Set(seen),isBranch:true});
      const isWinner=!hasWinner&&!signal.aborted&&(join.data.joinMode==='race'||output.state==='succeeded'&&join.data.joinMode==='first_success');
      if(isWinner){hasWinner=true;children.forEach((child,other)=>{if(other!==index)child.abort();});}
      return {id:branch.id,label:branch.label,...output,isWinner};
    }));
  } finally {signal.removeEventListener('abort',cancel);}
}

async function executePath(run,startId,context,signal,{stopAt=null,seen=new Set(),isBranch=false,single=false}={}) {
  let node=run.workflow.nodes.find(item=>item.id===startId);
  try {
    while(node) {
      if(signal.aborted)throw new DOMException('실행 취소','AbortError');
      if(node.id===stopAt)return {state:'succeeded',exitCode:0};
      if(seen.has(node.id))throw new Error('반복되는 실행 경로가 감지되었습니다.');
      seen.add(node.id);
      const result=run.steps[node.id];
      if(result.startedAt)throw new Error('병렬 분기가 합류 전 같은 단계에 도달했습니다.');
      run.activeNodeId=node.id;Object.assign(result,{state:'running',summary:`${node.data.label} 실행 중`,startedAt:new Date().toISOString()});publish(run);
      Object.assign(result,await executeWithPolicy(run,node,context,signal));
      result.finishedAt=new Date().toISOString();result.durationMs=Date.parse(result.finishedAt)-Date.parse(result.startedAt);publish(run);
      if(single||node.data.kind==='finish')return {state:stopAt?'failed':result.state,failedNodeId:result.state==='failed'?node.id:null,summary:stopAt?'합류 전에 종료되었습니다.':result.summary};
      if(node.data.kind==='parallel') {
        const outcomes=await executeParallel(run,node,context,signal,seen);
        result.branchResults=outcomes;context.parallelResults=outcomes;
        if(signal.aborted)throw new DOMException('실행 취소','AbortError');
        appendLog(run,result,'system',`병렬 분기 완료: ${outcomes.map(item=>`${item.label} ${executionLabels[item.state]}`).join(', ')}`);
        node=run.workflow.nodes.find(item=>item.id===node.data.joinId);continue;
      }
      const hasRoutes=resultRoutes(node.data).length>0;
      if(hasRoutes){result.selectedRoute=selectResultRoute(node.data,result);appendLog(run,result,'system',`선택 경로: ${result.selectedRoute.label} · ${result.selectedRoute.condition}`);}
      const port=hasRoutes?result.selectedRoute.id:node.data.kind==='condition'?result.branch:'out';
      const edge=run.workflow.edges.find(item=>item.source===node.id&&(item.sourceHandle||'out')===port);
      const next=run.workflow.nodes.find(item=>item.id===edge?.target);
      if(result.state==='failed') {
        const canContinue=hasRoutes||node.data.onError==='continue'||next?.data.kind==='condition'&&next.data.source==='exit_code';
        if(!canContinue)return {state:'failed',failedNodeId:node.id,exitCode:result.exitCode,summary:result.summary};
        result.isFailureHandled=true;appendLog(run,result,'system',hasRoutes?`실패 결과를 ${result.selectedRoute.label} 경로에서 처리합니다.`:'실패를 기록하고 다음 단계로 이동합니다.');
      }
      if(!next){result.state='failed';result.summary='다음 실행 경로가 없습니다.';return {state:'failed',failedNodeId:node.id,summary:result.summary};}
      run.visitedEdges.push(edge.id);node=next;
    }
    return {state:'failed',summary:'실행할 단계가 없습니다.'};
  } catch(error) {
    const state=error.name==='AbortError'?'cancelled':'failed',result=node&&run.steps[node.id];
    if(result&&result.state==='running'){result.state=state;result.summary=state==='cancelled'?'실행이 취소되었습니다.':error.message;result.finishedAt=new Date().toISOString();result.durationMs=Date.parse(result.finishedAt)-Date.parse(result.startedAt);appendLog(run,result,'system',result.summary);}
    return {state,failedNodeId:state==='failed'?node?.id:null,summary:error.message};
  }
}

export async function startWorkflowRun({job,workflow=initialWorkflow(job),scenario='success',stepId=null,input='0',isDraft=false,resultSamples={},groupName=null}) {
  if (controllers.has(job.id)) return getWorkflowRun(job.id);
  workflow=workflow||initialWorkflow(job);
  const previous=getWorkflowRuns(job.id),version=window.SupervisorJobVersions?.current(job.id);
  const savedGraph=version?initialWorkflow({...version.snapshot,id:job.id}):null;
  const versionMatches=version&&!isDraft&&executionDefinition(savedGraph)===executionDefinition(workflow);
  const controller = new AbortController();
  const startedAt = new Date().toISOString();
  const run = {id:`flow-${Date.now()}`,sequence:(previous[0]?.sequence||0)+1,versionNumber:versionMatches?version.number:null,sourceVersion:version?.number||null,versionLabel:versionMatches?`v${version.number}`:version?`v${version.number} · 미저장 초안`:'버전 미기록',jobSnapshot:window.SupervisorJobVersions?.snapshot({...job,workflow})||null,jobId:job.id,jobName:job.name,scope:stepId?'step':'flow',scenario,resultSamples:clone(resultSamples),state:'running',startedAt,finishedAt:null,durationMs:null,activeNodeId:null,failedNodeId:null,visitedEdges:[],definition:executionDefinition(workflow),workflow:clone(workflow),steps:Object.fromEntries(workflow.nodes.map(node=>[node.id,{id:node.id,label:node.data.label,kind:node.data.kind,state:'pending',startedAt:null,finishedAt:null,durationMs:null,exitCode:null,branch:null,summary:'아직 실행하지 않았습니다.',logs:[]}]))};
  run.groupName=groupName;
  controllers.set(job.id, controller);
  publish(run);
  window.SupervisorJobWorkspace.beginWorkflowRun?.(run);
  const context={exitCode:Number(input)};
  const start=workflow.nodes.find(node=>stepId?node.id===stepId:node.data.kind==='trigger');
  try {
    const outcome=await executePath(run,start?.id,context,controller.signal,{single:Boolean(stepId)});
    run.failedNodeId=outcome.failedNodeId||null;
    closeRun(run,outcome.state);
  } finally {controllers.delete(job.id);}
  return clone(run);
}

export function cancelWorkflowRun(jobId) { controllers.get(jobId)?.abort(); }

window.SupervisorWorkflowRuns = {start:startWorkflowRun,latest:getWorkflowRun,list:getWorkflowRuns,cancel:cancelWorkflowRun};
window.dispatchEvent(new CustomEvent('supervisor-workflow-history-ready'));
