import { ArrowRightLeft, Pause, Play, Plus, RotateCcw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState, type MouseEvent } from "react";
import { Badge, Button, DataTable, EmptyState, Field, IconButton, Panel, PanelHeader, TableCell } from "../../components/ui/primitives";
import type { OperationsClient, ResourcePage } from "../../services/operations-client";
import { useOperationsClient, usePolledResource, toErrorMessage } from "../../services/use-operations";
import type { ProcessConfigDto } from "../../services/wire-types";
import type { ProcessInstanceStatus, ProcessOperation, ProcessState, ProcessStatus } from "../../shared/types";

const PAGE_LIMIT = 50;
const POLL_INTERVAL_MS = 2000;
const stateTone: Record<ProcessState, "success" | "warning" | "danger" | "info" | "neutral"> = { starting: "info", running: "success", stopping: "warning", crashed: "danger", stopped: "neutral" };
const formatMemory = (bytes: number) => bytes === 0 ? "-" : `${(bytes / 1024 / 1024).toFixed(1)} MiB`;

type ProcessActionRunner = (name: string, action: () => Promise<unknown>) => void;

/** The regular GUI restart mirrors `msv restart`; rolling stays an explicit future action. */
export function restartProcess(client: Pick<OperationsClient, "restartProcess">, name: string): Promise<void> {
  return client.restartProcess(name);
}

export function confirmForceProcessRemoval(name: string): boolean {
  return window.confirm(`실행 중인 프로세스 \"${name}\"를 강제 종료하고 삭제합니다. 계속할까요?`);
}

export function ProcessLifecycleActions({
  client,
  process,
  pending,
  runAction,
}: {
  client: OperationsClient;
  process: ProcessStatus;
  pending: boolean;
  runAction: ProcessActionRunner;
}) {
  const stopPropagation = (event: MouseEvent) => event.stopPropagation();
  return <div className="flex gap-2"><IconButton label="시작" disabled={pending} onClick={(event) => { stopPropagation(event); runAction(process.name, () => client.startProcess(process.name)); }}><Play aria-hidden="true" size={15} /></IconButton><IconButton label="중지" disabled={pending} onClick={(event) => { stopPropagation(event); runAction(process.name, () => client.stopProcess(process.name)); }}><Pause aria-hidden="true" size={15} /></IconButton><IconButton label="재시작" disabled={pending} onClick={(event) => { stopPropagation(event); runAction(process.name, () => restartProcess(client, process.name)); }}><RotateCcw aria-hidden="true" size={15} /></IconButton><IconButton label="관리 모드 전환" disabled={pending} onClick={(event) => { stopPropagation(event); runAction(process.name, () => client.convertProcess(process.name, process.managementMode.type === "direct" ? "system_registered" : "direct", { autoStart: process.managementMode.type === "direct" })); }}><ArrowRightLeft aria-hidden="true" size={15} /></IconButton><IconButton label="삭제" disabled={pending} onClick={(event) => { stopPropagation(event); runAction(process.name, () => client.removeProcess(process.name, false)); }}><Trash2 aria-hidden="true" size={15} /></IconButton><IconButton label="강제 삭제" disabled={pending} onClick={(event) => { stopPropagation(event); if (!confirmForceProcessRemoval(process.name)) return; runAction(process.name, () => client.removeProcess(process.name, true)); }}><Trash2 aria-hidden="true" size={15} /></IconButton></div>;
}

export function ProcessesView() {
  const client = useOperationsClient();
  const [displayedPage, setDisplayedPage] = useState<ResourcePage<ProcessStatus> | null>(null);
  const [nextCursor, setNextCursor] = useState<string | undefined>();
  const [selectedProcess, setSelectedProcess] = useState<ProcessStatus | null>(null);
  const [instances, setInstances] = useState<ProcessInstanceStatus[]>([]);
  const [operation, setOperation] = useState<ProcessOperation | null>(null);
  const [desiredInstances, setDesiredInstances] = useState("1");
  const [actionError, setActionError] = useState<string | null>(null);
  const [pendingProcess, setPendingProcess] = useState<string | null>(null);
  const [formName, setFormName] = useState("api-server");
  const [formCommand, setFormCommand] = useState("pnpm dev");
  const [formCwd, setFormCwd] = useState("");
  const [managementMode, setManagementMode] = useState<"direct" | "system_registered">("direct");

  const listFirstPage = useCallback(() => client.listProcessesPage(undefined, undefined, PAGE_LIMIT), [client]);
  const { data: page, isLoading, errorMessage, refresh } = usePolledResource(listFirstPage, POLL_INTERVAL_MS);
  const processes = displayedPage?.records ?? [];
  useEffect(() => { setDisplayedPage(page ?? null); setNextCursor(page?.nextCursor); }, [page]);

  useEffect(() => {
    if (!selectedProcess) { setInstances([]); return; }
    let cancelled = false;
    void client.processInstances(selectedProcess.name).then((result) => { if (!cancelled) { setInstances(result.instances); setDesiredInstances(String(result.desiredInstances)); } }).catch((error) => { if (!cancelled) setActionError(toErrorMessage(error)); });
    return () => { cancelled = true; };
  }, [client, selectedProcess?.name]);

  const runAction = useCallback(async (name: string, action: () => Promise<unknown>) => {
    setActionError(null); setPendingProcess(name);
    try { const result = await action(); if (isProcessOperation(result)) setOperation(result); await refresh(); }
    catch (error) { setActionError(toErrorMessage(error)); } finally { setPendingProcess(null); }
  }, [refresh]);

  const selectProcess = (process: ProcessStatus) => {
    setSelectedProcess(process); setOperation(null); setFormName(process.name); setManagementMode(process.managementMode.type);
  };
  const saveProcess = async () => {
    if (!formName.trim() || !formCommand.trim()) { setActionError("이름과 명령은 필수입니다."); return; }
    const config: ProcessConfigDto = { name: formName.trim(), command: formCommand.trim(), ...(formCwd.trim() ? { cwd: formCwd.trim() } : {}), management_mode: managementMode === "direct" ? { type: "direct" } : { type: "system_registered", unit_name: `my-supervisor-managed-${formName.trim()}` } };
    await runAction(config.name, () => client.addProcess(config));
  };
  const loadNext = async () => {
    if (!nextCursor || !displayedPage) return;
    try { const next = await client.listProcessesPage(nextCursor, displayedPage.highWatermark, PAGE_LIMIT); setDisplayedPage(next); setNextCursor(next.nextCursor); if (next.partial) setActionError(`일부 파티션을 읽지 못했습니다: ${next.failedPartitions.join(", ")}`); }
    catch (error) { setActionError(toErrorMessage(error)); }
  };

  return <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_360px]"><div className="grid gap-5">
    <Panel><PanelHeader action={<Button variant="primary" disabled={pendingProcess !== null} onClick={() => { setSelectedProcess(null); setOperation(null); }}><Plus aria-hidden="true" size={16} />프로세스 추가</Button>} description="첫 50개와 명시적 다음 cursor만 요청합니다. 실패 시 마지막 성공 page를 유지합니다." title="프로세스" />
      {actionError || errorMessage || displayedPage?.partial ? <div className="border-b border-border bg-danger/10 px-4 py-2 text-xs font-medium text-danger">{actionError ?? errorMessage ?? `일부 파티션을 읽지 못했습니다: ${displayedPage?.failedPartitions.join(", ")}`}</div> : null}
      {isLoading ? <div className="p-4 text-sm text-muted">불러오는 중…</div> : processes.length === 0 ? <div className="p-4"><EmptyState title="등록된 프로세스가 없습니다" description="오른쪽 폼에서 macOS 프로세스를 추가하세요." /></div> : <DataTable columns={["이름", "상태", "관리 모드", "자원", "동작"]}>{processes.map((process) => <tr className="cursor-pointer hover:bg-surface" key={process.name} onClick={() => selectProcess(process)}><TableCell><div className="font-medium">{process.name}</div><div className="text-xs text-muted">재시작 {process.restartCount}회</div></TableCell><TableCell><Badge tone={stateTone[process.state]}>{process.state}</Badge></TableCell><TableCell><Badge tone={process.managementMode.type === "direct" ? "info" : "warning"}>{process.managementMode.type === "direct" ? "Direct" : "SystemRegistered"}</Badge></TableCell><TableCell><div className="text-xs">{process.cpuPercent.toFixed(1)}% CPU · {formatMemory(process.memoryBytes)}</div></TableCell><TableCell><ProcessLifecycleActions client={client} pending={pendingProcess === process.name} process={process} runAction={(name, action) => { void runAction(name, action); }} /></TableCell></tr>)}</DataTable>}
      {nextCursor ? <div className="border-t border-border p-3"><Button onClick={() => void loadNext()}>다음 50개</Button></div> : null}
    </Panel>
    {selectedProcess ? <Panel><PanelHeader title={`${selectedProcess.name} 인스턴스와 롤아웃`} description="실제 backend 결과만 표시합니다." /><div className="grid gap-3 p-4"><div className="flex items-end gap-2"><Field label="목표 인스턴스"><input className="h-9 w-24 rounded-md border border-border bg-surface px-3 text-sm" inputMode="numeric" onChange={(event) => setDesiredInstances(event.target.value)} value={desiredInstances} /></Field><Button disabled={pendingProcess === selectedProcess.name} onClick={() => { const target = Number(desiredInstances); if (!Number.isInteger(target) || target < 1) { setActionError("목표 인스턴스는 1 이상의 정수여야 합니다."); return; } void runAction(selectedProcess.name, () => client.scaleProcess(selectedProcess.name, target)); }}>스케일</Button></div>{instances.map((instance) => <div className="flex justify-between rounded border border-border p-2 text-xs" key={instance.instanceId}><span>#{instance.ordinal} generation {instance.generation}</span><Badge tone={stateTone[instance.state]}>{instance.state}</Badge></div>)}{operation ? <div className="rounded border border-info/30 bg-info/10 p-3 text-xs"><p className="font-medium">{operation.kind} · {operation.phase} · batch {operation.batch}</p>{operation.outcomes.map((outcome) => <p key={outcome.instanceId}>#{outcome.ordinal}: {outcome.state}{outcome.failedStage ? ` (${outcome.failedStage})` : ""}</p>)}</div> : null}</div></Panel> : null}
  </div><aside className="grid content-start gap-5"><Panel><PanelHeader title="macOS 프로세스 추가" description="SystemRegistered는 backend가 생성·등록하며, 수동 plist/타 OS 안내는 제공하지 않습니다." /><div className="grid gap-3 p-4"><Field label="이름"><input className="h-9 rounded-md border border-border bg-surface px-3 text-sm" onChange={(event) => setFormName(event.target.value)} value={formName} /></Field><Field label="명령"><input className="h-9 rounded-md border border-border bg-surface px-3 text-sm" onChange={(event) => setFormCommand(event.target.value)} value={formCommand} /></Field><Field label="작업 경로(선택)"><input className="h-9 rounded-md border border-border bg-surface px-3 text-sm" onChange={(event) => setFormCwd(event.target.value)} value={formCwd} /></Field><label className="grid gap-1 text-xs font-medium text-muted"><span>관리 모드</span><select className="h-9 rounded-md border border-border bg-surface px-3 text-sm" onChange={(event) => setManagementMode(event.target.value as "direct" | "system_registered")} value={managementMode}><option value="direct">Direct</option><option value="system_registered">SystemRegistered</option></select></label><Button variant="primary" disabled={pendingProcess !== null} onClick={() => void saveProcess()}>저장</Button></div></Panel></aside></div>;
}

function isProcessOperation(value: unknown): value is ProcessOperation { return Boolean(value && typeof value === "object" && "operationId" in value && "outcomes" in value); }
