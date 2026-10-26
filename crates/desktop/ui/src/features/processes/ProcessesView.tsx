import {MoreHorizontal, Play, Plus, RotateCcw, Square, Settings2, FileText, Search, X, Trash2} from "lucide-react";
import {useCallback, useEffect, useRef, useState} from "react";
import {Badge, Button, EmptyState, IconButton} from "../../components/ui/primitives";
import {useOperationsClient, usePolledResource, toErrorMessage} from "../../services/use-operations";
import type {OperationsClient} from "../../services/operations-client";
import type {ProcessConfigDto} from "../../services/wire-types";
import type {ProcessStatus, ProcessInstanceStatus} from "../../shared/types";
import {ProcessEditor, type ProcessEditorSection} from "./ProcessEditor";
import {LogRow} from "../logs/LogRows";
import type {LogEntry} from "../../services/log-mapping";
import {Modal} from "../../components/ui/Modal";
import {PathField} from "../../components/ui/PathField";
import "./process-controls.css";

export function restartProcess(client: Pick<OperationsClient, "restartProcess">, name: string) {return client.restartProcess(name);}
export function confirmForceProcessRemoval(name: string) {return window.confirm('실행 중인 프로세스 "' + name + '"를 강제 종료하고 삭제합니다. 계속할까요?');}
const states: Record<string, string> = {running: "실행 중", stopped: "중지됨", crashed: "종료 감지", starting: "시작 중", stopping: "중지 중"};
const guardStates: Record<string, string> = {healthy: "정상", unhealthy: "실패", unknown: "확인 대기", unsupported: "지원하지 않음"};
const restartCauses: Record<string, string> = {watch_changed: "파일 변경", memory_ceiling: "메모리 상한 초과", liveness_failure: "응답 확인 실패"};
type RunAction = (action: () => Promise<unknown>, notice: string) => void;

function LifecycleActions({process, isBusy, onAction}: {process: ProcessStatus; isBusy: boolean; onAction: RunAction}) {
  const client = useOperationsClient();
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
    <LifecycleActions process={process} isBusy={isBusy} onAction={onAction}/>
    <div ref={ref} className="overflow-anchor">
      <IconButton label={process.name + " 더 보기"} disabled={isBusy} aria-expanded={isOpen} onClick={() => setIsOpen(!isOpen)}><MoreHorizontal size={16}/></IconButton>
      {isOpen && <div className="overflow-menu">
        <button type="button" onClick={() => {setIsOpen(false); onEdit();}}><Settings2 size={14}/>설정 편집</button>
        <button type="button" className="text-danger" onClick={() => {
          const hasActiveProcess = process.state === "running" || process.state === "starting" || process.state === "stopping";
          if (!hasActiveProcess || confirmForceProcessRemoval(process.name)) {onAction(() => client.removeProcess(process.name, hasActiveProcess), process.name + " 등록을 삭제했습니다."); setIsOpen(false);}
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
        {process.guard?.lastRestartCause && <><dt>최근 감시 재시작 원인</dt><dd>{restartCauses[process.guard.lastRestartCause]}</dd></>}
      </dl>
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
  const client = useOperationsClient();
  const list = usePolledResource(useCallback(() => client.listProcesses(), [client]), 2000);
  const [selected, setSelected] = useState<string | null>(() => new URLSearchParams(location.search).get("selected"));
  const [tab, setTab] = useState("overview");
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [config, setConfig] = useState<ProcessConfigDto | null>(null);
  const [editor, setEditor] = useState<{initial: ProcessConfigDto; section: ProcessEditorSection; isNew: boolean} | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [isBusy, setIsBusy] = useState(false);
  const [instances, setInstances] = useState<ProcessInstanceStatus[]>([]);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [raw, setRaw] = useState<LogEntry | null>(null);
  const processes = list.data ?? [];
  const process = processes.find(item => item.name === selected);

  useEffect(() => {
    setConfig(null); setInstances([]);
    if (!selected) return;
    let isCancelled = false;
    Promise.all([client.getProcessConfig(selected), client.processInstances(selected)]).then(([value, result]) => {
      if (!isCancelled) {setConfig(value); setInstances(result.instances);}
    }).catch(error => {if (!isCancelled) setError(toErrorMessage(error));});
    return () => {isCancelled = true;};
  }, [selected, client, editor, process?.pid, process?.state]);

  useEffect(() => {
    if (!selected || tab !== "logs") return;
    let isCancelled = false;
    const read = () => client.queryLogs({kind: "process", source: selected, level: "all", grouping: "none", limit: 50}).then(result => {if (!isCancelled) setLogs(result.records.map(row => row.entry));}).catch(error => {if (!isCancelled) setError(toErrorMessage(error));});
    void read(); const interval = window.setInterval(() => void read(), 3000);
    return () => {isCancelled = true; clearInterval(interval);};
  }, [selected, tab, client]);

  async function act(action: () => Promise<unknown>, successNotice: string) {
    setIsBusy(true); setError(""); setNotice("");
    try {await action(); await list.refresh(); setNotice(successNotice);}
    catch (error) {setError(toErrorMessage(error));}
    finally {setIsBusy(false);}
  }
  async function openEditor(name: string, section: ProcessEditorSection = "execution") {
    try {
      const initial = selected === name && config ? config : await client.getProcessConfig(name);
      setSelected(name); setEditor({initial, section, isNew: false}); setError("");
    } catch (error) {setError(toErrorMessage(error));}
  }
  const runAction: RunAction = (action, successNotice) => void act(action, successNotice);
  const logsUrl = "?page=logs&kind=process&source=" + encodeURIComponent(process?.name ?? "");

  return <div className="process-workspace">
    <div className="workspace-heading"><div><h1>프로세스</h1><p>{processes.filter(item => item.state === "running").length}개 실행 중 · {processes.length}개 등록됨</p></div><Button variant="primary" onClick={() => setEditor({initial: {name: "", command: "node", args: [], cwd: "", env: {}, autostart: false, instances: 1}, section: "execution", isNew: true})}><Plus size={15}/>프로세스 추가</Button></div>
    {(error || list.errorMessage) && <p role="alert" className="workspace-alert">{error || list.errorMessage}</p>}
    {notice && <p role="status" className="workspace-notice">{notice}</p>}
    <div className={"process-split " + (process ? "has-detail" : "")}>
      <section className="workspace-panel">
        <div className="workspace-toolbar"><label className="search-box"><Search size={15}/><input aria-label="프로세스 검색" placeholder="프로세스 검색" value={query} onChange={event => setQuery(event.target.value)}/></label><select aria-label="프로세스 상태" value={filter} onChange={event => setFilter(event.target.value)}><option value="all">모든 상태</option><option value="running">실행 중</option><option value="stopped">중지됨</option><option value="crashed">종료 감지</option></select></div>
        <div className="process-list-head"><span>이름 / 상태</span><span>CPU</span><span>메모리</span><span className="process-action-heading">실행 제어</span></div>
        {processes.filter(item => item.name.includes(query) && (filter === "all" || item.state === filter)).map(item => <div className={"process-list-row " + (selected === item.name ? "is-selected" : "")} role="button" tabIndex={0} key={item.name} onClick={() => {setSelected(item.name); setTab("overview");}} onKeyDown={event => {if (event.key === "Enter" || event.key === " ") {event.preventDefault(); setSelected(item.name); setTab("overview");}}}>
          <div className="process-row-name"><strong title={item.name}>{item.name}</strong><small><i className={"state-dot " + item.state}/>{states[item.state]} · {item.managementMode.type === "direct" ? "직접 관리" : "시스템 서비스"}</small></div>
          <span>{item.cpuPercent.toFixed(1)}%</span><span>{(item.memoryBytes / 1048576).toFixed(1)} MiB</span>
          <Actions process={item} isBusy={isBusy} onAction={runAction} onEdit={() => void openEditor(item.name)}/>
        </div>)}
        {!processes.length && <div className="p-5"><EmptyState title="등록된 프로세스가 없습니다" description="실행할 프로그램을 추가해 관리하세요."/></div>}
      </section>
      {process && <aside className="workspace-panel process-detail">
        <header><div><h2>{process.name}</h2><small>{states[process.state]} · PID {process.pid ?? "—"}</small></div><IconButton label="상세 닫기" onClick={() => setSelected(null)}><X size={16}/></IconButton></header>
        <div className="process-detail-actions"><div><LifecycleActions process={process} isBusy={isBusy} onAction={runAction}/></div><Button className="process-settings-button" disabled={isBusy || !config} onClick={() => void openEditor(process.name)}><Settings2 size={14}/>설정 편집</Button></div>
        <div className="editor-tabs" aria-label="프로세스 상세 보기"><button type="button" aria-pressed={tab === "overview"} onClick={() => setTab("overview")}>개요</button><button type="button" aria-pressed={tab === "logs"} onClick={() => setTab("logs")}>로그</button></div>
        {tab === "overview" ? <div className="process-detail-body">
          <div className="metric-grid"><div><span>CPU</span><strong>{process.cpuPercent.toFixed(1)}%</strong></div><div><span>메모리</span><strong>{(process.memoryBytes / 1048576).toFixed(1)} MiB</strong></div><div><span>가동 시간</span><strong>{process.uptime}</strong></div><div><span>재시작</span><strong>{process.restartCount}회</strong></div></div>
          {config ? <>
            <dl className="detail-values"><dt>실행 명령</dt><dd><code>{config.command} {config.args?.join(" ")}</code></dd><dt>시작 시간</dt><dd>{process.startedAt ? new Date(process.startedAt).toLocaleString("ko-KR") : "—"}</dd><dt>인스턴스</dt><dd>{config.instances ?? 1}개</dd></dl>
            <PathField label="작업 디렉터리" value={config.cwd ?? ""} readOnly/>
            <div className="argument-heading"><strong>환경변수 {Object.keys(config.env ?? {}).length}개</strong><button type="button" onClick={() => void openEditor(process.name, "environment")}>환경변수 편집</button></div>
            <div className="process-environment-keys">{Object.keys(config.env ?? {}).map(key => <code key={key}>{key} <span className="text-muted">••••</span></code>)}</div>
            <PolicySummary process={process} config={config} onEdit={section => void openEditor(process.name, section)}/>
          </> : <p className="editor-note">설정을 불러오는 중입니다…</p>}
          <section className="process-summary-section"><div className="process-section-heading"><h3>인스턴스 상태</h3></div>{instances.map(slot => <div className="instance-row" key={slot.instanceId}><span>#{slot.ordinal} · PID {slot.pid ?? "—"}</span><Badge tone={slot.state === "running" ? "success" : "neutral"}>{states[slot.state]}</Badge></div>)}</section>
          <div className="process-detail-links">{config?.rolling && process.managementMode.type === "direct" && <Button disabled={isBusy} onClick={() => runAction(() => client.rollingRestartProcess(process.name), process.name + " 순차 재시작을 요청했습니다.")}><RotateCcw size={14}/>순차 재시작</Button>}<a href={logsUrl}><FileText size={14}/>전체 로그</a></div>
        </div> : <div className="process-log-preview">{logs.map(entry => <LogRow key={entry.id} entry={entry} onInspect={setRaw}/>)}<a href={logsUrl}>로그 탐색기에서 열기</a></div>}
      </aside>}
    </div>
    {editor && <ProcessEditor initial={editor.initial} initialSection={editor.section} isNew={editor.isNew} onClose={() => setEditor(null)} onSave={async next => {
      const result = editor.isNew ? await client.addProcess(next) : await client.updateProcessConfig(editor.initial.name, next, editor.initial);
      setSelected(result.name); setConfig(next); await list.refresh(); setNotice("프로세스 설정을 적용했습니다.");
    }}/>}
    {raw && <Modal title="로그 원본" onClose={() => setRaw(null)}><pre className="editor-body">{raw.raw.line}</pre></Modal>}
  </div>;
}
