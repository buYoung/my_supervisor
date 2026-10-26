import React, {memo, useCallback, useEffect, useMemo, useRef, useState} from 'react';
import {createRoot} from 'react-dom/client';
import {createPortal} from 'react-dom';
import {Background, ConnectionLineType, Handle, MarkerType, Position, ReactFlow, ReactFlowProvider, useReactFlow} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import {autoLayout, connectJobs, connectionError, definition, disconnectJobs, toDraft, toRecords, triggerLabel, validationErrors} from './graph-model.js';
import {WorkflowWorkspace} from './group-workflow.jsx';
import {FlowGroup} from './flow-group.jsx';

const bridge=window.SupervisorJobWorkspace;
const clone=value=>structuredClone(value);
const labels={succeeded:'성공',failed:'실패',pending:'대기',running:'실행 중',cancelled:'취소'};
const storage={read(key,fallback){try{return JSON.parse(sessionStorage.getItem(key))??fallback;}catch{return fallback;}},write(key,value){try{sessionStorage.setItem(key,JSON.stringify(value));}catch{}}};
const Icon=({name})=><span className="flow-icon" aria-hidden="true" dangerouslySetInnerHTML={{__html:bridge.icon(name)}}/>;
const ViewIcon=({flow})=><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">{flow?<><rect x="2" y="8" width="6" height="8" rx="1"/><rect x="16" y="2" width="6" height="7" rx="1"/><rect x="16" y="15" width="6" height="7" rx="1"/><path d="M8 12h4V5h4M12 12v7h4"/></>:<><path d="M8 5h13M8 12h13M8 19h13"/><path d="M3 5h1M3 12h1M3 19h1"/></>}</svg>;

const JobNode=memo(function JobNode({data,selected}) {
  const job=data.job;
  return <div className={`job-flow-node ${selected?'is-selected':''}`} data-job-node={job.id}>
    <Handle type="target" position={Position.Left} isConnectable={data.editing} aria-label={`${job.name} 선행 작업 연결`} title="선행 작업의 출력에서 여기로 연결"/>
    <div className="job-node-kind"><Icon name={job.triggerType==='depends_on'?'ArrowRight':'Clock3'}/><span>{job.triggerType==='depends_on'?'연결된 작업':'예약 작업'}</span><span className={`job-node-state ${job.state}`}><i/>{labels[job.state]||job.state}</span></div>
    <strong title={job.name}>{job.name||'이름 없는 작업'}</strong>
    <p>{triggerLabel(job)}</p>
    <footer><span>마지막 실행</span><span>{job.isNewMock?'기록 없음':job.last?.split(' · ').at(-1)||'—'}</span>{data.changed&&<b title="저장하지 않은 변경">편집됨</b>}</footer>
    <Handle type="source" position={Position.Right} isConnectable={data.editing} aria-label={`${job.name} 후속 작업 연결`} title="완료 후 실행할 작업의 입력에 연결"/>
  </div>;
});
const nodeTypes={job:JobNode,autoGroup:FlowGroup};
const {jobGroups,groupFrame}=window.SupervisorFlowGroups;

function ExecutionFields({job,onPatch,onInvalid}) {
  const container=useRef(null),picker=useRef(null),jobRef=useRef(job),callbacks=useRef({onPatch,onInvalid});
  jobRef.current=job;callbacks.current={onPatch,onInvalid};
  useEffect(()=>{
    const host=container.current;
    host.innerHTML='<div class="editor-field flow-command-field"><label for="flow-command">실행 명령</label><input id="flow-command" name="command" class="mono" autocomplete="off" spellcheck="false"><small>실행자와 인수를 한 줄 또는 항목별로 수정합니다.</small></div><details class="flow-environment"><summary>환경변수 <span data-env-size></span></summary><div class="flow-environment-actions"><button type="button" class="button small" data-env-open>파일 열기</button><input type="file" data-env-file hidden></div><textarea id="flow-environment" name="environment" spellcheck="false"></textarea><p class="field-error" data-env-error hidden></p></details>';
    const input=host.querySelector('[name="command"]'),environment=host.querySelector('[name="environment"]');
    input.value=job.command||'';environment.value=window.SupervisorEnvironment.serialize(job.environment||{});
    const environmentEditor=new window.SupervisorEnvironmentEditor({source:environment,icon:bridge.icon});
    const instance=new window.SupervisorExecutablePicker({input,icon:bridge.icon,cwd:()=>jobRef.current.cwd,binding:job.executable,notify:bridge.notify});picker.current=instance;
    let destroyed=false,hasInput=false;
    const commandInput=()=>{hasInput=true;callbacks.current.onPatch({command:instance.resolvedCommand(),executable:instance.bindingSnapshot()||null});};
    const runtimeChange=event=>{environmentEditor.setRuntime(event.detail.entry,event.detail.family);if(hasInput)callbacks.current.onPatch({executable:instance.bindingSnapshot()||null});};
    const environmentInput=()=>{const result=window.SupervisorEnvironment.parse(environment.value);const error=host.querySelector('[data-env-error]');error.hidden=!result.error;error.textContent=result.error||'';callbacks.current.onInvalid(result.error||'');if(!result.error){callbacks.current.onPatch({environment:result.environment});host.querySelector('[data-env-size]').textContent=`${Object.keys(result.environment).length}개`;}};
    input.addEventListener('input',commandInput);input.addEventListener('executablechange',runtimeChange);environment.addEventListener('input',environmentInput);
    host.querySelector('[data-env-size]').textContent=`${Object.keys(job.environment||{}).length}개`;
    host.querySelector('details').addEventListener('toggle',()=>environmentEditor.resizeValues());
    host.querySelector('[data-env-open]').onclick=()=>{const file=host.querySelector('[data-env-file]');file.value='';file.click();};
    host.querySelector('[data-env-file]').onchange=async event=>{
      const file=event.target.files[0];if(!file)return;
      if(file.size>512*1024){bridge.notify('512 KiB 이하의 환경변수 파일을 선택하세요.');return;}
      try{const incoming=window.SupervisorEnvironment.parse(await file.text()),local=window.SupervisorEnvironment.parse(environment.value);if(destroyed)return;if(incoming.error||local.error){bridge.notify(incoming.error||local.error);return;}
        new window.SupervisorEnvironment.EnvironmentMergeDialog({base:job.environment||{},local:local.environment,incoming:incoming.environment,fileName:file.name,icon:bridge.icon,onApply:values=>{environment.value=window.SupervisorEnvironment.serialize(values);environment.dispatchEvent(new Event('input',{bubbles:true}));environmentEditor.readSource();}});
      }catch{bridge.notify('환경변수 파일을 읽지 못했습니다.');}
    };
    return ()=>{destroyed=true;input.removeEventListener('input',commandInput);input.removeEventListener('executablechange',runtimeChange);environment.removeEventListener('input',environmentInput);instance.destroy();environmentEditor.destroy();picker.current=null;callbacks.current.onInvalid('');};
  },[job.id]);
  useEffect(()=>{picker.current?.schedule();},[job.cwd]);
  return <div ref={container} className="flow-execution-fields"/>;
}

function JobSettings({job,jobs,editing,dirty,onPatch,onConnect,onDisconnect,onClose,onSave,onInvalid,errors,onWorkflow,onRun,group,onGroupName}) {
  const [upstream,setUpstream]=useState('');
  const selectedErrors=errors.filter(error=>error.id===job.id);
  const update=(field,value)=>onPatch({[field]:value});
  const setTrigger=value=>onPatch({triggerType:value,triggerValue:value===job.triggerType?job.triggerValue:value==='cron'?'0 2 * * *':value==='interval'?'900':value==='one_shot'?'2026-10-27T02:00':'',upstream:value==='depends_on'?job.upstream:[]});
  return <aside className="job-settings" aria-label="작업 설정">
    <header><div><span>작업 설정</span><h2>{job.name||'이름 없는 작업'}</h2></div><button type="button" className="icon-button" onClick={onClose} aria-label="작업 설정 닫기"><Icon name="X"/></button></header>
    <form className="job-settings-form" onSubmit={event=>{event.preventDefault();onSave();}} noValidate>
      <div className="job-settings-body">
        <button className="button workflow-open-button" type="button" onClick={onWorkflow}><ViewIcon flow/><span><strong>흐름 편집</strong><small>실행 · 조건 · 동작 수정</small></span><Icon name="ArrowRight"/></button>
        <section className="flow-single-run"><button className="button small" type="button" disabled={job.state==='running'} onClick={onRun}><Icon name="Play"/>이 작업만 실행</button><small>저장된 설정으로 실행합니다. 선행 조건을 건너뛰고 후속 작업은 실행하지 않습니다.</small></section>
        {group&&<label className="flow-field"><span>연결 그룹 이름</span><input value={group.name} onChange={event=>onGroupName(event.target.value)}/><small>{group.nodeIds.length}개 작업 · 연결 관계에 따라 자동으로 묶입니다.</small></label>}
        <div className="flow-setting-state"><span className={`job-node-state ${job.state}`}><i/>{labels[job.state]||'대기'}</span><span>마지막 실행 결과 · 저장된 작업 정의</span></div>
        <label className="flow-field"><span>작업 이름</span><input name="name" value={job.name} onChange={event=>update('name',event.target.value)} aria-invalid={selectedErrors.some(error=>error.field==='name')} autoComplete="off"/></label>
        <section className="flow-setting-section"><h3>실행 조건</h3>
          <label className="flow-field"><span>시작 방식</span><select name="triggerType" value={job.triggerType} onChange={event=>setTrigger(event.target.value)}><option value="cron">예약 일정 · cron</option><option value="interval">일정 간격마다</option><option value="one_shot">지정 시각에 한 번</option><option value="depends_on">선행 작업 완료 후</option></select></label>
          {job.triggerType==='depends_on'?<div className="flow-dependencies"><p>{job.onDependencyFailure==='run_anyway'?'선행 작업이 모두 끝나면 성공 여부와 관계없이 실행합니다.':'선행 작업이 모두 성공하면 실행합니다.'}</p>{job.upstream.length?<ul>{job.upstream.map(id=><li key={id}><Icon name="ArrowRight"/><span>{jobs.find(item=>item.id===id)?.name||'없는 작업'}</span><button type="button" className="icon-button" aria-label={`${jobs.find(item=>item.id===id)?.name} 연결 해제`} onClick={()=>onDisconnect(id,job.id)}><Icon name="X"/></button></li>)}</ul>:<p className="flow-warning">선행 작업을 연결하거나 시작 방식을 바꾸세요.</p>}
            <div className="flow-add-dependency"><select aria-label="추가할 선행 작업" value={upstream} onChange={event=>setUpstream(event.target.value)}><option value="">선행 작업 선택</option>{jobs.filter(item=>item.id!==job.id&&!job.upstream.includes(item.id)).map(item=><option key={item.id} value={item.id}>{item.name}</option>)}</select><button className="button small" type="button" disabled={!upstream} onClick={()=>{if(onConnect(upstream,job.id))setUpstream('');}}>연결</button></div>
            {job.scheduleBeforeDependency&&<small className="flow-warning">연결로 기존 예약 일정이 선행 작업 조건으로 변경됩니다.</small>}
          </div>:<label className="flow-field"><span>{job.triggerType==='cron'?'cron 표현식':job.triggerType==='interval'?'실행 간격 (초)':'실행 시각'}</span><input name="triggerValue" className="mono" type={job.triggerType==='one_shot'?'datetime-local':job.triggerType==='interval'?'number':'text'} min={job.triggerType==='interval'?1:undefined} value={job.triggerValue} onChange={event=>update('triggerValue',event.target.value)}/><small>{job.triggerType==='cron'&&job.triggerValue==='0 2 * * *'?'매일 오전 2시 · ':''}{job.timezone}</small></label>}
        </section>
        <ExecutionFields job={job} onPatch={onPatch} onInvalid={onInvalid}/>
        <section className="flow-setting-section"><h3>실행 위치와 정책</h3>
          <label className="flow-field"><span>작업 디렉터리</span><div className="flow-directory"><input name="cwd" value={job.cwd} placeholder="기본 작업 디렉터리" onChange={event=>update('cwd',event.target.value)}/><button className="button small" type="button" aria-label="작업 디렉터리 선택" onClick={()=>new window.SampleFolderDialog({path:job.cwd,icon:bridge.icon,onChoose:path=>update('cwd',path)})}><Icon name="FolderOpen"/></button></div></label>
          <label className="flow-field"><span>이전 실행이 끝나지 않았다면</span><select name="policy" value={job.policy} onChange={event=>update('policy',event.target.value)}><option value="skip">이번 실행 건너뛰기</option><option value="queue">대기열에 추가</option><option value="parallel">동시에 실행</option></select></label>
          <label className="flow-field"><span>제한 시간 (초) <small>선택</small></span><input name="timeoutSeconds" type="number" min="1" value={job.timeoutSeconds} placeholder="제한 없음" onChange={event=>update('timeoutSeconds',event.target.value)}/></label>
          {job.triggerType==='depends_on'&&<label className="flow-field"><span>선행 작업 실패 시</span><select value={job.onDependencyFailure} onChange={event=>update('onDependencyFailure',event.target.value)}><option value="skip">이 작업 건너뛰기</option><option value="run_anyway">실패해도 실행</option></select></label>}
        </section>
        {selectedErrors.length>0&&<div className="flow-validation" role="alert">{selectedErrors.map((error,index)=><p key={index}>{error.message}</p>)}</div>}
      </div>
      <footer><small>{dirty?'저장 전까지 기존 설정이 유지됩니다.':'저장된 설정입니다.'}</small><button className="button primary" disabled={!dirty} type="submit">변경 저장</button></footer>
    </form>
  </aside>;
}

function JobFlowApp({switcher}) {
  const initial=useMemo(()=>toDraft(bridge.snapshot().jobs),[]);
  const savedDraft=useMemo(()=>storage.read('msv-mock-job-flow-draft',null),[]);
  const [saved,setSaved]=useState(initial);
  const [processes,setProcesses]=useState(()=>bridge.snapshot().processes);
  const [jobs,setJobs]=useState(()=>savedDraft?.base===definition(initial)?toDraft(savedDraft.records):initial);
  const [mode,setMode]=useState(()=>['flow','workflow'].includes(bridge.mode())?bridge.mode():'list');
  const [workflowId,setWorkflowId]=useState(()=>initial.find(job=>job.name===bridge.snapshot().selected)?.id||initial[0]?.id);
  const [editing,setEditing]=useState(false),[selected,setSelected]=useState(()=>initial.find(job=>job.name===bridge.snapshot().selected)?.id||null),[query,setQuery]=useState(''),[formRevision,setFormRevision]=useState(0);
  const [positions,setPositions]=useState(()=>({...autoLayout(initial),...storage.read('msv-mock-job-flow-layout',{})}));
  const [past,setPast]=useState([]),[future,setFuture]=useState([]),[message,setMessage]=useState(''),[errors,setErrors]=useState([]),[invalidInputs,setInvalidInputs]=useState({}),[conflict,setConflict]=useState(false);
  const canvasRef=useRef(null);
  const flow=useReactFlow(),jobsRef=useRef(jobs),savedRef=useRef(saved),saving=useRef(false),lastEdit=useRef({});
  jobsRef.current=jobs;savedRef.current=saved;
  const dirty=definition(jobs)!==definition(saved);
  const selectedJob=jobs.find(job=>job.id===selected);
  const workflowJob=jobs.find(job=>job.id===workflowId)||jobs[0];
  const openWorkflow=useCallback(id=>{const job=jobsRef.current.find(item=>item.id===id);if(!job)return;setWorkflowId(id);setMode('workflow');bridge.select(job.name);},[]);
  useEffect(()=>{const open=event=>{const job=jobsRef.current.find(item=>item.name===event.detail.name);if(!job)return;if(event.detail.runId){const url=new URL(location.href);url.searchParams.set('scope','job');url.searchParams.set('panel','runs');url.searchParams.set('run',event.detail.runId);url.searchParams.set('runView',event.detail.runView||'flow');url.searchParams.delete('step');history.replaceState(history.state,'',url);}openWorkflow(job.id);};window.addEventListener('supervisor-job-workflow-open',open);return ()=>window.removeEventListener('supervisor-job-workflow-open',open);},[openWorkflow]);
  useEffect(()=>{const open=event=>{const job=jobsRef.current.find(item=>item.name===event.detail.name);if(job){setSelected(job.id);const url=new URL(location.href);url.searchParams.set('scope','group');history.replaceState(history.state,'',url);openWorkflow(job.id);}};window.addEventListener('supervisor-job-connections-open',open);return ()=>window.removeEventListener('supervisor-job-connections-open',open);},[]);
  const clearSelection=useCallback(()=>{setSelected(null);bridge.select(null);},[]);
  useEffect(()=>{if(mode!=='flow')return;window.addEventListener('supervisor-job-flow-clear',clearSelection);return ()=>window.removeEventListener('supervisor-job-flow-clear',clearSelection);},[clearSelection,mode]);
  const change=useCallback((next,key='')=>{
    const current=jobsRef.current;if(definition(next)===definition(current))return;
    if(!key||lastEdit.current.key!==key||Date.now()-lastEdit.current.time>650)setPast(items=>[...items.slice(-49),clone(current)]);
    lastEdit.current={key,time:Date.now()};setFuture([]);setErrors([]);setMessage('');jobsRef.current=next;setJobs(next);
  },[]);
  const patch=useCallback((id,fields)=>change(jobsRef.current.map(job=>job.id===id?{...job,...fields}:job),`${id}:${Object.keys(fields).join(',')}`),[change]);
  const connect=useCallback((source,target)=>{const error=connectionError(jobsRef.current,source,target);if(error){setMessage(error);return false;}const next=connectJobs(jobsRef.current,source,target),layout=autoLayout(next);change(next);setPositions(layout);storage.write('msv-mock-job-flow-layout',layout);setSelected(target);setMessage('선행 작업을 연결했습니다. 저장하면 적용됩니다.');return true;},[change]);
  const disconnect=useCallback((source,target)=>{const next=disconnectJobs(jobsRef.current,source,target),layout=autoLayout(next);change(next);setPositions(layout);storage.write('msv-mock-job-flow-layout',layout);setMessage('연결을 해제했습니다. 저장하면 적용됩니다.');},[change]);
  const undo=()=>{if(!past.length)return;const previous=past.at(-1);setFuture(items=>[clone(jobs),...items]);setPast(items=>items.slice(0,-1));jobsRef.current=previous;setJobs(previous);setPositions(autoLayout(previous));storage.write('msv-mock-job-flow-layout',autoLayout(previous));setErrors([]);setInvalidInputs({});setMessage('이전 편집으로 되돌렸습니다.');setFormRevision(value=>value+1);};
  const redo=()=>{if(!future.length)return;const next=future[0];setPast(items=>[...items,clone(jobs)]);setFuture(items=>items.slice(1));jobsRef.current=next;setJobs(next);setPositions(autoLayout(next));storage.write('msv-mock-job-flow-layout',autoLayout(next));setErrors([]);setInvalidInputs({});setMessage('편집을 다시 적용했습니다.');setFormRevision(value=>value+1);};
  const save=value=>{
    const records=Array.isArray(value)?value:jobsRef.current;
    const issues=validationErrors(records);for(const [id,message] of Object.entries(invalidInputs))if(message)issues.push({id,field:'environment',message});
    if(issues.length){setErrors(issues);setSelected(issues[0].id);setMessage('입력을 확인한 뒤 저장하세요.');bridge.notify(issues[0].message);return false;}
    if(conflict){setMessage('목록에서 변경된 최신 설정을 먼저 불러오세요.');return false;}
    saving.current=true;bridge.commit(toRecords(records));saving.current=false;
    const current=toDraft(bridge.snapshot().jobs);setSaved(current);setJobs(current);jobsRef.current=current;setPast([]);setFuture([]);setMessage('작업 설정과 의존 관계를 저장했습니다.');storage.write('msv-mock-job-flow-draft',null);
    return true;
  };
  const discard=()=>{setJobs(clone(saved));jobsRef.current=clone(saved);setPast([]);setFuture([]);setConflict(false);setErrors([]);setInvalidInputs({});setMessage('저장된 설정으로 되돌렸습니다.');setFormRevision(value=>value+1);};
  useEffect(()=>bridge.subscribe(()=>{
    if(saving.current)return;
    setProcesses(bridge.snapshot().processes);
    const next=toDraft(bridge.snapshot().jobs);setPositions(current=>({...autoLayout(next),...current}));const hasDraft=definition(jobsRef.current)!==definition(savedRef.current);
    if(!hasDraft){setSaved(next);setJobs(next);jobsRef.current=next;}else if(definition(next)!==definition(savedRef.current)){setConflict(true);setSaved(next);}else{const runtime=new Map(next.map(job=>[job.id,job]));setJobs(current=>current.map(job=>({...job,state:runtime.get(job.id)?.state||job.state,last:runtime.get(job.id)?.last||job.last})));}
  }),[]);
  useEffect(()=>{bridge.setMode(mode);},[mode]);
  useEffect(()=>{storage.write('msv-mock-job-flow-draft',dirty?{base:definition(saved),records:toRecords(jobs)}:null);},[jobs,saved,dirty]);
  useEffect(()=>{const canvas=canvasRef.current;if(mode!=='flow'||!canvas)return;const observer=new ResizeObserver(()=>requestAnimationFrame(()=>flow.fitView({padding:.18,maxZoom:1})));observer.observe(canvas);return ()=>observer.disconnect();},[mode]);
  useEffect(()=>{if(mode==='flow')requestAnimationFrame(()=>flow.fitView({padding:.18,maxZoom:1,duration:0}));},[mode]);
  useEffect(()=>{
    const add=event=>{if(mode!=='flow'||!event.target.closest('[data-open-editor="job"]'))return;event.preventDefault();event.stopImmediatePropagation();
      let index=1;while(jobsRef.current.some(job=>job.name===`new-job-${index}`))index++;
      const job={id:crypto.randomUUID(),name:`new-job-${index}`,command:'',cwd:'',environment:{},triggerType:'cron',triggerValue:'0 2 * * *',policy:'skip',timezone:'Asia/Seoul',timeoutSeconds:'',onDependencyFailure:'skip',state:'pending',upstream:[],isNewMock:true,last:'기록 없음'};
      const next=[...jobsRef.current,job],layout=autoLayout(next);change(next);setPositions(items=>({...items,[job.id]:layout[job.id]}));setSelected(job.id);requestAnimationFrame(()=>flow.fitView({nodes:[{id:job.id}],padding:.5,maxZoom:1}));
    };document.addEventListener('click',add,true);return ()=>document.removeEventListener('click',add,true);
  },[mode,change]);
  const selectedIds=useMemo(()=>new Set([selected,...(selectedJob?.upstream||[]),...jobs.filter(job=>job.upstream.includes(selected)).map(job=>job.id)]),[selected,jobs,selectedJob]);
  const groups=useMemo(()=>jobGroups(jobs),[jobs]);
  const groupsRef=useRef(groups);groupsRef.current=groups;
  const renameGroup=(group,name)=>change(jobsRef.current.map(job=>group.nodeIds.includes(job.id)?{...job,connectionGroupName:name}:job),`group:${group.id}`);
  const nodes=useMemo(()=>[
    ...groups.map(group=>{const frame=groupFrame(group,positions,230,122);return {id:group.id,type:'autoGroup',position:frame.position,width:frame.width,height:frame.height,style:{width:frame.width,height:frame.height},zIndex:-1,selectable:false,draggable:true,deletable:false,dragHandle:'.flow-group-header',data:{name:group.name,count:group.nodeIds.length,unit:'작업',onRename:name=>renameGroup(group,name)}};}),
    ...jobs.map(job=>({id:job.id,type:'job',width:230,height:122,position:positions[job.id]||{x:0,y:0},selected:job.id===selected,draggable:true,deletable:false,zIndex:2,ariaLabel:`${job.name}, ${triggerLabel(job)}, ${labels[job.state]||'대기'}`,data:{job,editing,changed:definition([job])!==definition(saved.filter(item=>item.id===job.id))},style:{opacity:query&&!job.name.toLowerCase().includes(query.toLowerCase()) ? .3 : 1}}))
  ],[jobs,groups,positions,selected,editing,query,saved]);
  const edges=useMemo(()=>jobs.flatMap(job=>job.upstream.map(id=>({id:`${id}::${job.id}`,source:id,target:job.id,type:'smoothstep',deletable:editing,label:job.onDependencyFailure==='run_anyway'?'완료 후':'성공 후',markerEnd:{type:MarkerType.ArrowClosed,width:16,height:16,color:selectedIds.has(id)&&selectedIds.has(job.id)?'var(--accent)':'var(--line-strong)'},style:{stroke:selectedIds.has(id)&&selectedIds.has(job.id)?'var(--accent)':'var(--line-strong)',strokeWidth:1.6},labelStyle:{fill:'var(--muted)',fontSize:10},labelBgStyle:{fill:'var(--canvas)'},ariaLabel:`${jobs.find(item=>item.id===id)?.name} 완료 후 ${job.name}`}))),[jobs,editing,selectedIds]);
  const selectNode=useCallback((event,node)=>{if(!node.data.job)return;setSelected(node.id);setErrors([]);bridge.select(node.data.job.name);},[]);
  const validConnection=useCallback(connection=>!connectionError(jobsRef.current,connection.source,connection.target),[]);
  const onNodesChange=useCallback(changes=>{
    const movements=changes.filter(item=>item.type==='position'&&item.position);
    if(movements.length)setPositions(current=>{
      const next={...current};
      for(const item of movements){
        const group=groupsRef.current.find(group=>group.id===item.id);
        if(group){const frame=groupFrame(group,next,230,122),dx=item.position.x-frame.position.x,dy=item.position.y-frame.position.y;for(const id of group.nodeIds)next[id]={x:(next[id]?.x||0)+dx,y:(next[id]?.y||0)+dy};}
        else next[item.id]=item.position;
      }
      storage.write('msv-mock-job-flow-layout',next);return next;
    });
    for(const item of changes)if(item.type==='select'&&item.selected&&!groupsRef.current.some(group=>group.id===item.id))setSelected(item.id);
  },[]);
  const onInvalid=useCallback(error=>setInvalidInputs(values=>({...values,[selected]:error})),[selected]);
  const changeMode=next=>{setMode(next);setMessage('');};
  return <>
    {createPortal(<div className="job-view-switch" role="group" aria-label="작업 보기 방식"><button type="button" aria-pressed={mode==='list'} onClick={()=>changeMode('list')}><ViewIcon/>목록</button><button type="button" aria-pressed={mode==='flow'} onClick={()=>changeMode('flow')}><ViewIcon flow/>작업 간 연결</button></div>,switcher)}
    {mode==='workflow'&&workflowJob&&<WorkflowWorkspace key={workflowJob.id} job={workflowJob} jobs={jobs} processes={processes} ExecutionFields={ExecutionFields} onSaveJobs={save} onBack={()=>{const url=new URL(location.href);for(const key of ['scope','panel','run','runView','step','version','base'])url.searchParams.delete(key);history.replaceState(history.state,'',url);setMode('list');}} onOuter={()=>{setSelected(workflowJob.id);setMode('flow');bridge.select(workflowJob.name);}} onSelectJob={openWorkflow}/>}
    {mode==='flow'&&<section className="job-flow-workspace" aria-label="작업 흐름">
      <div className="job-flow-toolbar"><div className="job-flow-heading"><strong>작업 간 연결</strong><span>{jobs.length}개 작업 · {edges.length}개 연결</span></div><label className="flow-search"><Icon name="Search"/><input aria-label="흐름에서 작업 검색" placeholder="작업 찾기" value={query} onChange={event=>setQuery(event.target.value)}/></label><div className="flow-toolbar-actions"><button className="icon-button" type="button" disabled={!past.length} onClick={undo} aria-label="흐름 편집 실행 취소"><Icon name="ArrowLeft"/></button><button className="icon-button" type="button" disabled={!future.length} onClick={redo} aria-label="흐름 편집 다시 실행"><Icon name="ArrowRight"/></button><span className="flow-toolbar-divider"/><button className="button small" type="button" onClick={()=>{const next=autoLayout(jobs);setPositions(next);storage.write('msv-mock-job-flow-layout',next);requestAnimationFrame(()=>flow.fitView({padding:.18,maxZoom:1}));}}>자동 정렬</button><button className={`button small ${editing?'is-active':''}`} type="button" aria-pressed={editing} onClick={()=>setEditing(value=>!value)}><Icon name="Settings2"/>연결 편집</button><button className="button primary small" type="button" disabled={!dirty||conflict} onClick={save}>변경 저장{dirty&&<span className="unsaved-dot"/>}</button></div></div>
      <div className={`job-flow-layout ${selectedJob?'has-settings':''}`}>
        <div ref={canvasRef} className={`job-flow-canvas ${editing?'is-editing':''}`} data-testid="job-flow-canvas">
          <div className="flow-canvas-caption"><span>{editing?'작업 간 연결 편집':'작업 간 의존 관계'}</span><p>{editing?'오른쪽 연결점을 다른 작업의 왼쪽 연결점에 놓으세요.':'드래그로 이동 · 그룹 이름 수정 · 더블클릭으로 흐름 편집'}</p></div>
          <ReactFlow nodes={nodes} edges={edges} nodeTypes={nodeTypes} onNodeClick={selectNode} onNodeDoubleClick={(event,node)=>{if(node.data.job)openWorkflow(node.id);}} onNodesChange={onNodesChange} onPaneClick={clearSelection} onConnect={connection=>connect(connection.source,connection.target)} isValidConnection={validConnection} onEdgesDelete={removed=>{let next=jobsRef.current;for(const edge of removed)next=disconnectJobs(next,edge.source,edge.target);change(next);}} nodesConnectable={editing} nodesDraggable edgesReconnectable={false} connectionLineType={ConnectionLineType.SmoothStep} fitView fitViewOptions={{padding:.18,maxZoom:1}} minZoom={.35} maxZoom={1.6} deleteKeyCode={editing?['Backspace','Delete']:null} panOnScroll zoomOnDoubleClick={false} proOptions={{hideAttribution:false}} ariaLabelConfig={{'node.a11yDescription.default':'Enter로 선택하고 방향키로 이동합니다.','edge.a11yDescription.default':'연결 편집 중 Delete 키로 연결을 해제합니다.'}} onInit={()=>requestAnimationFrame(()=>flow.fitView({padding:.18,maxZoom:1}))}>
            <Background gap={24} size={1} color="var(--line)"/>
          </ReactFlow>
          {!jobs.length&&<div className="flow-empty"><h3>아직 등록된 작업이 없습니다</h3><p>작업을 추가한 뒤 실행 순서를 연결하세요.</p></div>}
          <div className="flow-canvas-controls"><button type="button" aria-label="흐름 축소" onClick={()=>flow.zoomOut()}>−</button><button type="button" aria-label="흐름 확대" onClick={()=>flow.zoomIn()}>+</button><button type="button" aria-label="전체 작업 보기" onClick={()=>flow.fitView({padding:.18,maxZoom:1})}><Icon name="Search"/></button></div>
          <div className="flow-legend"><span><i className="succeeded"/>성공</span><span><i className="failed"/>실패</span><span><i className="pending"/>대기</span><em>색상은 작업별 마지막 실행 결과입니다.</em></div>
        </div>
        {selectedJob?<JobSettings key={`${selectedJob.id}-${formRevision}`} job={selectedJob} jobs={jobs} editing={editing} dirty={dirty&&!conflict} onPatch={fields=>patch(selectedJob.id,fields)} onConnect={connect} onDisconnect={disconnect} onClose={clearSelection} onSave={save} onInvalid={onInvalid} errors={errors} onWorkflow={()=>openWorkflow(selectedJob.id)} onRun={()=>bridge.runSingle(selectedJob.name)} group={groups.find(group=>group.nodeIds.includes(selectedJob.id))} onGroupName={name=>renameGroup(groups.find(group=>group.nodeIds.includes(selectedJob.id)),name)}/>:<aside className="flow-selection-placeholder"><ViewIcon flow/><h2>작업을 선택하세요</h2><p>실행 조건과 명령을 확인하고,<br/>선행 작업을 연결할 수 있습니다.</p><div><strong>연결을 읽는 방법</strong><span>선행 작업 <Icon name="ArrowRight"/> 후속 작업</span><small>여러 선행 작업을 연결하면 모두 완료될 때까지 기다립니다.</small></div></aside>}
      </div>
      <footer className="job-flow-status"><span role="status">{conflict?'목록에서 설정이 변경되었습니다. 최신 설정을 불러온 뒤 편집하세요.':message|| (dirty?'저장하지 않은 변경이 있습니다.':'작업 정의를 편집합니다. 실행은 기존 설정을 따릅니다.')}</span>{(dirty||conflict)&&<button type="button" onClick={discard}>{conflict?'최신 설정 불러오기':'변경 되돌리기'}</button>}<span className="flow-keyboard-hint">{editing?'Delete 연결 해제 · ':''}스크롤 이동 · Ctrl + 휠 확대</span></footer>
    </section>}
  </>;
}

if(bridge&&document.body.dataset.page==='jobs'){
  const head=document.querySelector('.head-actions');
  const switcher=document.createElement('div');head.prepend(switcher);
  const host=document.createElement('div');host.id='job-flow-root';document.querySelector('#normal-view').before(host);
  createRoot(host).render(<ReactFlowProvider><JobFlowApp switcher={switcher}/></ReactFlowProvider>);
}
