import {RawLogInspector} from "../logs/RawLogInspector";
import {logEntryUrl} from "../logs/log-navigation";
import {useSessionValue} from "../../shared/editor-session";
import {useDisplayTime} from "../../shared/display-time";
import {MoreHorizontal, Play, Plus, RotateCcw, Square, Settings2, FileText, Search, X, Trash2} from "lucide-react";
import {useCallback, useEffect, useRef, useState} from "react";
import {Badge, Button, EmptyState, IconButton} from "../../components/ui/primitives";
import {useOperationsClient, useProcessResource, useResource, refreshResources, toErrorMessage} from "../../services/use-operations";
import type {OperationsClient} from "../../services/operations-client";
import type {ProcessConfigDto} from "../../services/wire-types";
import type {ProcessStatus, ProcessOperation} from "../../shared/types";
import {ProcessEditor, type ProcessEditorSection} from "./ProcessEditor";
import {LogRow} from "../logs/LogRows";
import type {LogEntry} from "../../services/log-mapping";
import {ResourceState} from "../../components/ui/ResourceState";
import {OperationResult} from "./OperationResult";
import {PathField} from "../../components/ui/PathField";
import "./process-controls.css";

export function restartProcess(client: Pick<OperationsClient, "restartProcess">, name: string) {return client.restartProcess(name);}
export function confirmForceProcessRemoval(name: string) {return window.confirm('실행 중인 프로세스 "' + name + '"를 강제 종료하고 삭제합니다. 계속할까요?');}
const states: Record<string, string> = {running: "실행 중", stopped: "중지됨", crashed: "종료 감지", starting: "시작 중", stopping: "중지 중"};
const guardStates: Record<string, string> = {healthy: "정상", unhealthy: "실패", unknown: "확인 대기", unsupported: "지원하지 않음"};
const restartCauses: Record<string, string> = {watch_changed: "파일 변경", memory_ceiling: "메모리 상한 초과", liveness_failure: "응답 확인 실패"};
type RunAction = (action: () => Promise<unknown>, notice: string) => void;

export function ProcessLifecycleActions({client, process, isBusy, onAction}: {client: Pick<OperationsClient, "startProcess" | "stopProcess" | "restartProcess">; process: ProcessStatus; isBusy: boolean; onAction: RunAction}) {
  const canStop = process.state === "running" || process.state === "starting";
  const isTransitioning = process.state === "starting" || process.state === "stopping";
  const isSystemManaged = process.managementMode.type === "system_registered";
  return <>
    {canStop || process.state === "stopping" ?
      <Button disabled={isBusy || process.state === "stopping"} aria-label={process.name + " 중지"} onClick={() => onAction(() => client.stopProcess(process.name), process.name + " 중지 요청을 반영했습니다.")}><Square size={14}/>{process.state === "stopping" ? "중지 중" : "중지"}</Button> :
      <Button disabled={isBusy} aria-label={process.name + " 시작"} onClick={() => onAction(() => client.startProcess(process.name), process.name + " 시작 요청을 반영했습니다.")}><Play size={14}/>시작</Button>}
    {canStop && <Button disabled={isBusy || isTransitioning || isSystemManaged} title={isSystemManaged ? "시스템 서비스의 재시작은 macOS가 관리합니다." : "프로세스를 중지한 뒤 다시 시작합니다."} aria-label={process.name + " 재시작"} onClick={() => onAction(() => client.restartProcess(process.name), process.name + " 재시작 요청을 반영했습니다.")}><RotateCcw size={14}/>재시작</Button>}
  </>;
}

export function getProcessRemovalAction(client: Pick<OperationsClient, "removeProcess">, process: Pick<ProcessStatus, "name" | "state">) {
  const hasActiveProcess = process.state === "running" || process.state === "starting" || process.state === "stopping";
  if (hasActiveProcess ? !confirmForceProcessRemoval(process.name) : !window.confirm(`중지된 ${process.name}의 등록과 저장된 설정을 삭제할까요?`)) return null;
  return () => client.removeProcess(process.name, hasActiveProcess);
}

function Actions({process, isBusy, onAction, onEdit}: {process: ProcessStatus; isBusy: boolean; onAction: RunAction; onEdit: () => void}) {
  const client = useOperationsClient();
  const [isOpen, setIsOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const closeOutside = (event: PointerEvent) => {if (!ref.current?.contains(event.target as Node)) setIsOpen(false);};
    const closeEscape = (event: KeyboardEvent) => {if (event.key === "Escape" && isOpen) {setIsOpen(false); ref.current?.querySelector("button")?.focus();}};
    document.addEventListener("pointerdown", closeOutside); document.addEventListener("keydown", closeEscape);
    return () => {document.removeEventListener("pointerdown", closeOutside); document.removeEventListener("keydown", closeEscape);};
  }, [isOpen]);
  return <div className="process-actions" onClick={event => event.stopPropagation()} onKeyDown={event => {if (event.key === "Enter" || event.key === " ") event.stopPropagation();}}>
    <ProcessLifecycleActions client={client} process={process} isBusy={isBusy} onAction={onAction}/>
    <div ref={ref} className="overflow-anchor">
      <IconButton label={process.name + " 더 보기"} disabled={isBusy} aria-expanded={isOpen} onClick={() => setIsOpen(!isOpen)}><MoreHorizontal size={16}/></IconButton>
      {isOpen && <div className="overflow-menu">
        <button type="button" onClick={() => {setIsOpen(false); onEdit();}}><Settings2 size={14}/>설정 편집</button>
        <button type="button" className="text-danger" onClick={() => {
          const action = getProcessRemovalAction(client, process);
          if (action) {onAction(action, process.name + " 등록을 삭제했습니다."); setIsOpen(false);}
        }}><Trash2 size={14}/>등록 삭제</button>
      </div>}
    </div>
  </div>;
}

function PolicySummary({process, config, onEdit}: {process: ProcessStatus; config: ProcessConfigDto; onEdit: (section: ProcessEditorSection) => void}) {
  const isSystemManaged = config.management_mode?.type === "system_registered";
  const isExitRestartEnabled = config.restart?.enabled ?? true;
  const hasRestartCondition = isExitRestartEnabled || Boolean(config.watch || config.memory || config.liveness);
  const hasReachedRestartLimit = !isSystemManaged && hasRestartCondition && (config.instances ?? 1) === 1
    && config.restart?.max_retries != null && process.restartCount >= config.restart.max_retries
    && (process.state === "stopped" || process.state === "crashed");
  return <>
    <section className="process-summary-section">
      <div className="process-section-heading"><h3>자동 재시작</h3><button type="button" onClick={() => onEdit("restart")}>조건 편집</button></div>
      <p className="section-description">직접 ‘중지’를 누르면 자동으로 다시 시작하지 않습니다.</p>
      <dl className="policy-summary-list">
        <dt>프로세스 종료</dt><dd><span className={isExitRestartEnabled ? "policy-enabled" : "text-muted"}>{isExitRestartEnabled ? "감지 후 재시작" : "종료 상태 유지"}</span></dd>
        {!isSystemManaged && <>
          <dt>파일 변경</dt><dd>{config.watch ? "감시 경로 " + config.watch.roots.length + "개" : "사용 안 함"}</dd>
          <dt>메모리 상한</dt><dd>{config.memory ? config.memory.ceiling_bytes / 1048576 + " MiB 초과 시" : "사용 안 함"}</dd>
          <dt>응답 확인 실패</dt><dd>{config.liveness ? config.liveness.consecutive_failures + "회 연속 실패 시" : "사용 안 함"}</dd>
        </>}
      </dl>
      <p className="section-description">{!hasRestartCondition ? "자동 재시작 조건이 모두 꺼져 있습니다." : isSystemManaged ? "재시작은 macOS가 관리합니다." : "재시작 전 " + (config.restart?.backoff_initial_ms ?? 1000) / 1000 + "초 대기 · " + (config.restart?.max_retries == null ? "횟수 제한 없음" : "최대 " + config.restart.max_retries + "회")}</p>
      {hasReachedRestartLimit && <p className="restart-limit-notice" role="status">자동 재시작 한도 {config.restart?.max_retries}회에 도달했습니다. 다시 실행하려면 ‘시작’을 누르세요.</p>}
    </section>
    <section className="process-summary-section">
      <div className="process-section-heading"><h3>상태 확인</h3><button type="button" onClick={() => onEdit("health")}>검사 설정</button></div>
      <dl className="policy-summary-list">
        <dt>응답 확인</dt><dd>{config.liveness ? guardStates[process.guard?.liveness ?? "unknown"] : "사용 안 함"}</dd>
        <dt>준비 상태</dt><dd>{config.readiness ? guardStates[process.guard?.readiness ?? "unknown"] : "사용 안 함"}</dd>
        {process.guard?.lastRestartCause && <><dt>최근 감시 재시작 원인</dt><dd>{restartCauses[process.guard.lastRestartCause]??process.guard.lastRestartCause}</dd></>}
      </dl>
      {process.guard?.lastError&&<p role="alert" className="text-danger">최근 상태 검사 오류: {process.guard.lastError}</p>}
      {process.guard?.isHistorical && <p className="section-description">이전 실행의 기록입니다. 현재 상태를 다시 확인하는 중입니다.</p>}
      <p className="section-description">응답 확인 실패는 재시작으로 이어집니다. 준비 상태 실패는 재시작하지 않습니다.</p>
    </section>
    <section className="process-summary-section">
      <div className="process-section-heading"><h3>관리·종료</h3><button type="button" onClick={() => onEdit("management")}>관리 방식 변경</button></div>
      <dl className="policy-summary-list"><dt>관리 주체</dt><dd>{isSystemManaged ? "macOS 시스템 서비스" : "Supervisor"}</dd><dt>자동 시작</dt><dd>{config.autostart ? "데몬 시작 시 실행" : "직접 시작"}</dd><dt>앱 종료 후</dt><dd>{isSystemManaged || config.lifecycle === "detached" ? "계속 실행" : "함께 종료"}</dd></dl>
    </section>
  </>;
}

export function ProcessesView() {
  const {dateTime} = useDisplayTime();
  const client = useOperationsClient();
  const list = useProcessResource();
  const [selected, setSelected] = useSessionValue<string|null>("process.selected",null,new URLSearchParams(location.search).get("selected")??undefined);
  const [tab, setTab] = useSessionValue("process.tab","overview");
  const [query, setQuery] = useSessionValue("process.query","");
  const [filter, setFilter] = useSessionValue("process.filter","all");

  const detailElement=useRef<HTMLElement>(null),returnFocus=useRef<HTMLElement|null>(null),shouldFocusDetail=useRef(false);
  function showDetail(name:string,focus=false,origin?:HTMLElement){returnFocus.current=origin??document.activeElement as HTMLElement;shouldFocusDetail.current=focus;setSelected(name);setTab("overview");if(focus&&selected===name)detailElement.current?.querySelector<HTMLElement>("h2")?.focus({preventScroll:true});}
  function closeDetail(){setSelected(null);queueMicrotask(()=>returnFocus.current?.focus());}
  const [editor, setEditor] = useState<{initial: ProcessConfigDto; section: ProcessEditorSection; isNew: boolean} | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy,setBusy]=useState<Record<string,boolean>>({});const activeActions=useRef(new Set<string>());
  const [actionMessages,setActionMessages]=useState<Record<string,{error:boolean;text:string}>>({});
  const [operations,setOperations]=useState<Record<string,ProcessOperation>>({});const rollingIds=useRef<Record<string,string>>({});
  const [raw, setRaw] = useState<LogEntry | null>(null);
  const processes = list.data ?? [];
  const resourceKey="process:"+JSON.stringify(selected);
  const selectedStatus=useResource(resourceKey+":status",()=>selected?client.getProcess(selected):Promise.resolve(null),selected?2000:0);
  const configuration=useResource(resourceKey+":config",()=>selected?client.getProcessConfig(selected):Promise.resolve(null),selected?5000:0);
  const instanceResource=useResource(resourceKey+":instances",()=>selected?client.processInstances(selected):Promise.resolve(null),selected?2000:0);
  const logResource=useResource(resourceKey+":logs:"+tab,()=>selected&&tab==="logs"?client.queryLogs({kind:"process",source:selected,level:"all",grouping:"none",limit:50}):Promise.resolve(null),selected&&tab==="logs"?3000:0);
  const process=selectedStatus.data??processes.find(item=>item.name===selected);
  useEffect(()=>{if(!selected)return;detailElement.current?.scrollIntoView({block:"nearest"});if(shouldFocusDetail.current){detailElement.current?.querySelector<HTMLElement>('h2')?.focus({preventScroll:true});shouldFocusDetail.current=false;}},[selected,Boolean(process)]);
  const config=configuration.data;
  const instances=instanceResource.data?.instances??[];
  const logs=logResource.data?.records.map(row=>row.entry)??[];
  async function act(name:string,action:()=>Promise<unknown>,successNotice:string){
    if(activeActions.current.has(name))return;activeActions.current.add(name);setBusy(value=>({...value,[name]:true}));setError("");
    setActionMessages(current=>{const next={...current};delete next[name];return next;});
    try{const result=await action();const operation=result&&typeof result==="object"&&"outcomes" in result?result as ProcessOperation:undefined;
      if(operation)setOperations(current=>({...current,[name]:operation}));
      const hasFailure=operation&&(operation.phase==="failed_closed"||operation.outcomes.some(item=>item.state!=="completed"));
      setActionMessages(current=>({...current,[name]:{error:Boolean(hasFailure),text:hasFailure?name+" 작업에 실패하거나 시도하지 않은 항목이 있습니다.":successNotice}}));
      await refreshResources(client,key=>key==="processes"||key==="daemon"||key.startsWith("process:"+JSON.stringify(name)+":"));
    }catch(cause){setActionMessages(current=>({...current,[name]:{error:true,text:name+": "+toErrorMessage(cause)}}));}
    finally{activeActions.current.delete(name);setBusy(value=>({...value,[name]:false}));}
  }
  const actionFor=(name:string):RunAction=>(action,message)=>void act(name,action,message);
  const roll=(name:string)=>{const operationId=rollingIds.current[name]??crypto.randomUUID();rollingIds.current[name]=operationId;void act(name,async()=>{const result=await client.rollingRestartProcess(name,operationId);delete rollingIds.current[name];return result;},name+" 순차 재시작을 완료했습니다.");};
  const selection=useRef(selected);selection.current=selected;const opening=useRef(0);
  useEffect(()=>()=>{opening.current++;},[]);
  async function openEditor(name: string, section: ProcessEditorSection = "execution") {
    const request=++opening.current;setSelected(name);
    try {
      const initial = selected === name && config ? config : await client.getProcessConfig(name);
      if(request===opening.current&&selection.current===name){setEditor({initial, section, isNew: false});setError("");}
    } catch (error) {setError(toErrorMessage(error));}
  }
  const logsUrl = logEntryUrl({kind:"process",source:process?.name??""});

  return <div className="process-workspace">
    <div className="workspace-heading"><div><h1>프로세스</h1><p>{list.data===null?"목록 확인 중":`${processes.filter(item => item.state === "running").length}개 실행 중 · ${processes.length}${list.hasMore?"+":""}개 확인${list.isStale?" · 마지막 확인 값":""}`}</p></div><Button variant="primary" onClick={() => setEditor({initial: {name: "", command: "node", args: [], cwd: "", env: {}, autostart: false, instances: 1}, section: "execution", isNew: true})}><Plus size={15}/>프로세스 추가</Button></div>
    {error&&<p role="alert" className="workspace-alert">{error}</p>}<ResourceState resource={list} label="프로세스 목록"/>{Object.entries(actionMessages).map(([name,message])=><p key={name} role={message.error?"alert":"status"} className={message.error?"workspace-alert":"workspace-notice"}>{message.text}{processes.some(item=>item.name===name)&&<Button onClick={event=>showDetail(name,true,event.currentTarget)}>대상 보기</Button>}</p>)}
    {notice && <p role="status" className="workspace-notice">{notice}</p>}
    <div className={"process-split " + (process ? "has-detail" : "")}>
      <section className="workspace-panel">
        <div className="workspace-toolbar"><label className="search-box"><Search size={15}/><input aria-label="프로세스 검색" placeholder="프로세스 검색" value={query} onChange={event => setQuery(event.target.value)}/></label><select aria-label="프로세스 상태" value={filter} onChange={event => setFilter(event.target.value)}><option value="all">모든 상태</option><option value="running">실행 중</option><option value="stopped">중지됨</option><option value="crashed">종료 감지</option></select></div>
        <div className="process-list-head"><span>이름 / 상태</span><span>CPU</span><span>메모리</span><span className="process-action-heading">실행 제어</span></div>
        {processes.filter(item => item.name.includes(query) && (filter === "all" || item.state === filter)).map(item => <div className={"process-list-row " + (selected === item.name ? "is-selected" : "")} role="group" aria-label={item.name+" 프로세스 선택"} tabIndex={0} data-process-row={item.name} key={item.name} onClick={event => showDetail(item.name,false,event.currentTarget)} onKeyDown={event => {if(event.target!==event.currentTarget)return;if (event.key === "Enter" || event.key === " ") {event.preventDefault(); showDetail(item.name,false,event.currentTarget);}}}>
          <div className="process-row-name"><strong title={item.name}>{item.name}</strong><small><i className={"state-dot " + item.state}/>{states[item.state]} · {item.managementMode.type === "direct" ? "직접 관리" : "시스템 서비스"}</small></div>
          <span>{item.cpuPercent.toFixed(1)}%</span><span>{(item.memoryBytes / 1048576).toFixed(1)} MiB</span>
          <div className="flex gap-1" onClick={event=>event.stopPropagation()} onKeyDown={event=>event.stopPropagation()}><IconButton label={item.name+" 상세 보기"} aria-pressed={selected===item.name} onClick={event=>showDetail(item.name,true,event.currentTarget)}><FileText size={14}/></IconButton><Actions process={item} isBusy={Boolean(busy[item.name])} onAction={actionFor(item.name)} onEdit={() => void openEditor(item.name)}/></div>
        </div>)}
        {list.data!==null&&!list.errorMessage&&!list.partial&&!processes.length && <div className="p-5"><EmptyState title="등록된 프로세스가 없습니다" description="실행할 프로그램을 추가해 관리하세요."/></div>}
        {processes.length>0&&!processes.some(item=>item.name.includes(query)&&(filter==="all"||item.state===filter))&&<div className="p-5"><p>조건에 맞는 프로세스가 없습니다.</p><Button onClick={()=>{setQuery("");setFilter("all");}}>검색·필터 초기화</Button></div>}
        {list.hasMore&&<div className="p-5"><p>현재 불러온 {processes.length}개에서 검색합니다.</p><Button disabled={list.isRefreshing} onClick={()=>void list.loadMore()}>프로세스 더 불러오기</Button></div>}
      </section>
      {selected&&!process&&<aside className="workspace-panel"><ResourceState resource={selectedStatus} label="선택한 프로세스"/></aside>}
      {process && <aside ref={detailElement} className="workspace-panel process-detail">
        <header><div><h2 tabIndex={-1}>{process.name}</h2><small>{states[process.state]} · PID {process.pid ?? "—"}</small></div><IconButton label="상세 닫기" onClick={closeDetail}><X size={16}/></IconButton></header>
        <div className="process-detail-actions"><div><ProcessLifecycleActions client={client} process={process} isBusy={Boolean(busy[process.name])||Boolean(selectedStatus.errorMessage)} onAction={actionFor(process.name)}/></div><Button className="process-settings-button" disabled={Boolean(busy[process.name]) || !config} onClick={() => void openEditor(process.name)}><Settings2 size={14}/>설정 편집</Button></div>
        <div className="editor-tabs" aria-label="프로세스 상세 보기"><button type="button" aria-pressed={tab === "overview"} onClick={() => setTab("overview")}>개요</button><button type="button" aria-pressed={tab === "logs"} onClick={() => setTab("logs")}>로그</button></div>
        {tab === "overview" ? <div className="process-detail-body"><ResourceState resource={selectedStatus} label="프로세스 상태"/><ResourceState resource={configuration} label="프로세스 설정"/>{operations[process.name]&&<OperationResult operation={operations[process.name]} instances={instances}/>}
          <div className="metric-grid"><div><span>CPU</span><strong>{process.cpuPercent.toFixed(1)}%</strong></div><div><span>메모리</span><strong>{(process.memoryBytes / 1048576).toFixed(1)} MiB</strong></div><div><span>가동 시간</span><strong>{process.uptime}</strong></div><div><span>재시작</span><strong>{process.restartCount}회</strong></div></div>
          {config ? <>
            <dl className="detail-values"><dt>실행 명령</dt><dd><code>{config.command} {config.args?.join(" ")}</code></dd><dt>시작 시간</dt><dd>{process.startedAt ? dateTime(process.startedAt) : "—"}</dd><dt>인스턴스</dt><dd>{config.instances ?? 1}개</dd></dl>
            <PathField label="작업 디렉터리" value={config.cwd ?? ""} readOnly/>
            <div className="argument-heading"><strong>환경변수 {Object.keys(config.env ?? {}).length}개</strong><button type="button" onClick={() => void openEditor(process.name, "environment")}>환경변수 편집</button></div>
            <div className="process-environment-keys">{Object.keys(config.env ?? {}).map(key => <code key={key}>{key} <span className="text-muted">••••</span></code>)}</div>
            <PolicySummary process={process} config={config} onEdit={section => void openEditor(process.name, section)}/>
          </> : <p className="editor-note">설정을 불러오는 중입니다…</p>}
          <section className="process-summary-section"><div className="process-section-heading"><h3>인스턴스 상태</h3></div><ResourceState resource={instanceResource} label="인스턴스"/>{instances.map(slot => <div className="instance-row" key={slot.instanceId}><span>#{slot.ordinal} · PID {slot.pid ?? "—"}</span><Badge tone={slot.state === "running" ? "success" : "neutral"}>{states[slot.state]??`알 수 없는 상태 (${slot.state})`}</Badge></div>)}</section>
          <div className="process-detail-links">{config?.rolling && process.managementMode.type === "direct" && <Button disabled={Boolean(busy[process.name])} onClick={()=>roll(process.name)}><RotateCcw size={14}/>{rollingIds.current[process.name]?"순차 재시작 응답 확인":"순차 재시작"}</Button>}<a href={logsUrl}><FileText size={14}/>전체 로그</a></div>
        </div> : <div className="process-log-preview"><ResourceState resource={logResource} label="프로세스 로그"/>{logResource.data!==null&&!logResource.errorMessage&&!logs.length&&<p>기록된 로그가 없습니다.</p>}{logs.map(entry => <LogRow key={entry.id} entry={entry} onInspect={setRaw}/>)}<a href={logsUrl}>로그 탐색기에서 열기</a></div>}
      </aside>}
    </div>
    {editor && <ProcessEditor initial={editor.initial} initialSection={editor.section} isNew={editor.isNew} onClose={() => setEditor(null)} onSave={async (next,expected) => {
      const result = editor.isNew ? await client.addProcess(next) : await client.updateProcessConfig(editor.initial.name, next, expected??editor.initial);
      setSelected(result.name); await refreshResources(client,key=>key==="processes"||key==="daemon"||key.startsWith("process:"+JSON.stringify(result.name)+":")); setNotice("프로세스 설정을 적용했습니다.");
    }}/>}
    {raw && <RawLogInspector entry={raw} onClose={()=>setRaw(null)}/>}
  </div>;
}
