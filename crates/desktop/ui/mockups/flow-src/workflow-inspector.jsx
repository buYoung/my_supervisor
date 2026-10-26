import React, {useEffect,useRef,useState} from 'react';
import {durationLabel,executionLabels} from './workflow-runner.js';
import {stepKinds,stepSummary} from './workflow-model.js';

const icon=name=><span className="flow-icon" aria-hidden="true" dangerouslySetInnerHTML={{__html:window.SupervisorJobWorkspace.icon(name)}}/>;
const timeLabel=value=>value?new Date(value).toLocaleTimeString('ko-KR',{hour12:false,hour:'2-digit',minute:'2-digit',second:'2-digit'}):'—';

export function WorkflowInspectorHeader({step,tab,onTabChange,onClose,readOnly=false}) {
  return <><header><div><span>{stepKinds[step.data.kind].label}</span><h2>{step.data.label}</h2></div><button type="button" className="icon-button" aria-label="단계 패널 닫기" onClick={onClose}>{icon('X')}</button></header>{!readOnly&&<div className="workflow-inspector-tabs" role="tablist" aria-label="단계 정보">{(readOnly?[['result','실행 결과']]:[['settings','단계 설정'],['result','실행 결과']]).map(([value,label])=><button key={value} type="button" role="tab" aria-selected={value===tab} tabIndex={value===tab?0:-1} onClick={()=>onTabChange(value)} onKeyDown={event=>{if(event.key==='ArrowLeft'||event.key==='ArrowRight'){event.preventDefault();onTabChange(value==='settings'?'result':'settings');event.currentTarget.parentElement.querySelector('[aria-selected="false"]')?.focus();}}}>{label}</button>)}</div>}</>;
}

export function StepExecution({step,run,onClose,onTabChange,onRun,disabled,isStale,onSelectStep,isSnapshot=false}) {
  const result=run?.steps[step.id],logRef=useRef(null);
  const [filter,setFilter]=useState('all'),[isFollowing,setIsFollowing]=useState(true),[now,setNow]=useState(Date.now());
  const logs=(result?.logs||[]).filter(log=>filter==='all'||log.stream==='stderr');
  useEffect(()=>{if(result?.state!=='running')return;const timer=setInterval(()=>setNow(Date.now()),250);return ()=>clearInterval(timer);},[result?.state]);
  useEffect(()=>{if(isFollowing&&logRef.current)logRef.current.scrollTop=logRef.current.scrollHeight;},[result?.logs.length,isFollowing,filter]);
  const duration=result?.state==='running'?now-Date.parse(result.startedAt):result?.durationMs;
  const snapshot=run?.workflow.nodes.find(node=>node.id===step.id)?.data;
  return <aside className="job-settings workflow-step-settings workflow-result-panel" aria-label="단계 실행 결과">
    <WorkflowInspectorHeader step={step} tab="result" onTabChange={onTabChange} onClose={onClose} readOnly={isSnapshot}/>
    <div className="workflow-result-body">
      {isStale&&<p className="workflow-result-note">편집 전 실행 기록입니다. 현재 설정과 다를 수 있습니다.</p>}
      {result?<><div className={`step-result-summary ${result.state}`}><span className="execution-state"><i/>{executionLabels[result.state]}</span><p>{result.summary}</p></div>
        {result.selectedRoute&&<div className="command-route-result"><span>선택한 결과 경로</span><strong>{result.selectedRoute.label}</strong><small>{result.selectedRoute.condition}</small>{result.isFailureHandled&&<small>이 실패 결과는 선택한 경로에서 처리했습니다.</small>}</div>}
        <details className="execution-metadata"><summary>실행 정보 <span>{durationLabel(duration)} · 종료 {result.exitCode??'—'}{result.maxAttempts>1?` · ${result.attempt}/${result.maxAttempts}회`: ''}</span></summary><dl className="step-result-facts"><div><dt>실행 범위</dt><dd>{run.scope==='step'?'이 단계만':'전체 흐름'}</dd></div><div><dt>소요 시간</dt><dd>{durationLabel(duration)}</dd></div><div><dt>시작 시각</dt><dd>{timeLabel(result.startedAt)}</dd></div><div><dt>종료 코드</dt><dd>{result.exitCode??'—'}</dd></div>{result.maxAttempts>1&&<div><dt>시도 횟수</dt><dd>{result.attempt}/{result.maxAttempts}</dd></div>}{result.branch&&<div><dt>선택 경로</dt><dd>{result.branch==='true'?'참':'거짓'}</dd></div>}</dl>
        {snapshot&&<div className="step-executed-value"><span>실행 당시 설정</span><code>{snapshot.kind==='command'?snapshot.command:snapshot.kind==='process'?`${snapshot.processName} · ${{start:'시작',stop:'중지',restart:'재시작'}[snapshot.operation]}`:snapshot.kind==='condition'?snapshot.source==='exit_code'?`종료 코드 ${snapshot.operator==='equals'?'=':snapshot.operator==='not_equals'?'≠':'≥'} ${snapshot.value}`:snapshot.path:stepSummary(snapshot)}</code></div>}
        </details>{result.state==='skipped'&&run.failedNodeId&&<button type="button" className="text-button" onClick={()=>onSelectStep(run.failedNodeId)}>실패한 단계 보기 {icon('ArrowRight')}</button>}
      </>:<div className="step-result-empty"><h3>아직 실행 결과가 없습니다</h3><p>흐름을 실행하면 단계별 결과와 출력 로그가 여기에 표시됩니다.</p></div>}
      {result?.branchResults&&<section className="parallel-results"><h3>분기별 결과</h3>{result.branchResults.map(branch=><div key={branch.id} className={branch.isWinner?'is-winner':''}><span>{branch.label}{branch.isWinner&&<small>채택한 결과</small>}</span><b>{executionLabels[branch.state]}</b></div>)}</section>}
      <section className="step-log-section"><header><h3>실행 로그 <span>{result?.logs.length||0}</span></h3><div role="group" aria-label="로그 표시"><button type="button" aria-pressed={filter==='all'} onClick={()=>setFilter('all')}>전체</button><button type="button" aria-pressed={filter==='stderr'} onClick={()=>setFilter('stderr')}>오류</button></div></header>
        <div className="step-log-lines" role="log" aria-live="polite" aria-label={`${step.data.label} 실행 로그`} ref={logRef}>{logs.length?logs.map((log,index)=><div className={`step-log-line ${log.stream}`} key={`${index}-${log.time}`}><time>{timeLabel(log.time)}</time><span>{log.stream==='system'?'시스템':log.stream}</span><code>{log.message}</code></div>):<p>{result?.state==='running'?'출력을 기다리고 있습니다.':filter==='stderr'?'오류 로그가 없습니다.':result?.state==='skipped'?'실행하지 않은 단계입니다.':'표시할 로그가 없습니다.'}</p>}</div>
        <label className="step-log-follow"><input type="checkbox" checked={isFollowing} onChange={event=>setIsFollowing(event.target.checked)}/>새 로그 따라가기</label>
      </section>
    </div>
    <footer className="workflow-result-footer"><span className="mono">{run?.id||'실행 전'} · {run?.versionLabel||'목업'}</span>{!isSnapshot&&step.data.kind!=='trigger'&&<button type="button" className="button small" disabled={disabled} onClick={onRun}>{icon('Play')}이 단계만 실행</button>}</footer>
  </aside>;
}

export function WorkflowRunSummary({run,onSelectStep,onCancel,isStale}) {
  const [now,setNow]=useState(Date.now());
  useEffect(()=>{if(run?.state!=='running')return;const timer=setInterval(()=>setNow(Date.now()),250);return ()=>clearInterval(timer);},[run?.state]);
  if(!run)return <div className="workflow-run-summary is-empty"><span>최근 실행 없음</span><small>흐름을 실행하면 노드에서 진행 상태를 확인할 수 있습니다.</small></div>;
  const current=run.steps[run.activeNodeId],failed=run.steps[window.SupervisorRunPresentation.summary(run).focusNodeId];
  const count=Object.values(run.steps).filter(step=>['succeeded','failed','cancelled'].includes(step.state)).length;
  return <div className={`workflow-run-summary ${run.state}`} role="region" aria-label="흐름 실행 상태"><span className="execution-state" role="status"><i/>{executionLabels[run.state]}</span><strong aria-live="polite">{run.state==='running'?(run.activeNodeIds?.length>1?`${run.activeNodeIds.length}개 단계 동시 실행`:current?.label||'실행 준비 중'):run.state==='failed'?failed?.label||'흐름 실행 실패':run.state==='cancelled'?'실행이 취소되었습니다.':run.scope==='step'?'단계 단독 실행 완료':'흐름 실행 완료'}</strong><span>{count}개 실행 · {durationLabel(run.state==='running'?now-Date.parse(run.startedAt):run.durationMs)}</span>{isStale&&<small>편집 전 기록</small>}<div>{current&&<button type="button" onClick={()=>onSelectStep(current.id)}>현재 단계 보기</button>}{run.state==='failed'&&failed&&<button type="button" onClick={()=>onSelectStep(failed.id)}>실패 단계 보기</button>}{run.state==='running'&&<button type="button" onClick={onCancel}>실행 취소</button>}</div></div>;
}
