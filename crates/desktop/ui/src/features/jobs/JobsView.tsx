import { Play, RotateCcw, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Badge, Button, DataTable, EmptyState, Field, IconButton, Panel, PanelHeader, TableCell } from "../../components/ui/primitives";
import { useOperationsClient, usePolledResource, toErrorMessage } from "../../services/use-operations";
import type { ResourcePage } from "../../services/operations-client";
import type { JobConfigDto } from "../../services/wire-types";
import type { JobRun, JobRunState, JobStatus, JobTrigger } from "../../shared/types";

const JOBS_POLL_INTERVAL_MS = 2000;
const PAGE_LIMIT = 50;
const RUN_HISTORY_LIMIT = 50;

const runTone: Record<JobRunState, "success" | "warning" | "danger" | "info" | "neutral"> = {
  pending: "warning", running: "info", succeeded: "success", failed: "danger", timed_out: "danger", cancelled: "neutral", skipped: "neutral",
};

function describeTrigger(trigger: JobTrigger) {
  if (trigger.type === "cron") return `cron ${trigger.expr}`;
  if (trigger.type === "interval") return `${trigger.everySec}s interval`;
  if (trigger.type === "one_shot") return trigger.at;
  return `depends on ${trigger.jobs.join(", ")}`;
}

export function JobsView() {
  const client = useOperationsClient();
  const [nextCursor, setNextCursor] = useState<string | undefined>();
  const [displayedPage, setDisplayedPage] = useState<ResourcePage<JobStatus> | null>(null);
  const [selectedJob, setSelectedJob] = useState<JobStatus | null>(null);
  const [jobRuns, setJobRuns] = useState<JobRun[]>([]);
  const [runsError, setRunsError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [pendingJob, setPendingJob] = useState<string | null>(null);
  const [formName, setFormName] = useState("nightly-backup");
  const [formCommand, setFormCommand] = useState("/usr/local/bin/backup");
  const [triggerType, setTriggerType] = useState<JobTrigger["type"]>("cron");
  const [triggerValue, setTriggerValue] = useState("0 2 * * *");
  const [formOverlap, setFormOverlap] = useState<"skip" | "queue" | "parallel">("skip");

  const listFirstPage = useCallback(() => client.listJobsPage(undefined, undefined, PAGE_LIMIT), [client]);
  const { data: page, isLoading, errorMessage, refresh } = usePolledResource(listFirstPage, JOBS_POLL_INTERVAL_MS);
  const jobs = displayedPage?.records ?? [];

  useEffect(() => {
    setNextCursor(page?.nextCursor);
    setDisplayedPage(page ?? null);
    setSelectedJob((current) => current && jobs.find((job) => job.name === current.name) ? jobs.find((job) => job.name === current.name) ?? null : null);
  }, [page]);

  useEffect(() => {
    if (!selectedJob) {
      setJobRuns([]);
      setRunsError(null);
      return;
    }
    let cancelled = false;
    setRunsError(null);
    void client.listRuns(selectedJob.name, RUN_HISTORY_LIMIT).then((result) => {
      if (!cancelled) setJobRuns(result.runs);
    }).catch((error) => {
      if (!cancelled) setRunsError(toErrorMessage(error));
    });
    return () => { cancelled = true; };
  }, [client, selectedJob?.name]);

  const runJobAction = useCallback(async (name: string, action: () => Promise<unknown>) => {
    setActionError(null); setPendingJob(name);
    try { await action(); await refresh(); } catch (error) { setActionError(toErrorMessage(error)); } finally { setPendingJob(null); }
  }, [refresh]);

  const buildTrigger = (): JobTrigger | null => {
    const value = triggerValue.trim();
    if (!value) return null;
    if (triggerType === "cron") return { type: "cron", expr: value };
    if (triggerType === "interval") {
      const everySec = Number(value);
      return Number.isInteger(everySec) && everySec > 0 ? { type: "interval", everySec } : null;
    }
    if (triggerType === "one_shot") return { type: "one_shot", at: value };
    const jobs = value.split(",").map((name) => name.trim()).filter(Boolean);
    return jobs.length > 0 ? { type: "depends_on", jobs } : null;
  };

  const handleSaveJob = useCallback(async () => {
    const trigger = buildTrigger();
    if (!formName.trim() || !formCommand.trim() || !trigger) { setActionError("이름, 명령과 유효한 트리거 값은 필수입니다."); return; }
    const config: JobConfigDto = { name: formName.trim(), command: formCommand.trim(), trigger, on_overlap: formOverlap };
    await runJobAction(config.name, () => selectedJob?.name === config.name ? client.updateJob(config.name, config) : client.addJob(config));
  }, [client, formCommand, formName, formOverlap, selectedJob?.name, triggerType, triggerValue, runJobAction]);

  const selectJob = (job: JobStatus) => {
    setSelectedJob(job); setFormName(job.name); setFormOverlap(job.onOverlap); setTriggerType(job.trigger.type);
    setTriggerValue(job.trigger.type === "cron" ? job.trigger.expr : job.trigger.type === "interval" ? String(job.trigger.everySec) : job.trigger.type === "one_shot" ? job.trigger.at : job.trigger.jobs.join(", "));
  };

  const loadNextPage = async () => {
    if (!nextCursor || !displayedPage) return;
    setActionError(null);
    try {
      const next = await client.listJobsPage(nextCursor, displayedPage.highWatermark, PAGE_LIMIT);
      setDisplayedPage(next);
      setNextCursor(next.nextCursor);
      // Keep the existing page visible when the next bounded request fails; the UI deliberately does not fan out.
      if (next.partial) setActionError(`일부 파티션을 읽지 못했습니다: ${next.failedPartitions.join(", ")}`);
    } catch (error) { setActionError(toErrorMessage(error)); }
  };

  return <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_360px]">
    <div className="grid gap-5">
      <Panel>
        <PanelHeader action={<Button variant="primary" disabled={pendingJob !== null} onClick={() => { setSelectedJob(null); setJobRuns([]); }}>새 작업</Button>} description="한 번의 50개 page 요청만 사용하며, 실행 이력은 선택한 작업에서만 조회합니다." title="작업" />
        {actionError || errorMessage || displayedPage?.partial ? <div className="border-b border-border bg-danger/10 px-4 py-2 text-xs font-medium text-danger">{actionError ?? errorMessage ?? `일부 파티션을 읽지 못했습니다: ${displayedPage?.failedPartitions.join(", ")}`}</div> : null}
        {isLoading ? <div className="px-4 py-6 text-sm text-muted">불러오는 중…</div> : jobs.length === 0 ? <div className="p-4"><EmptyState title="등록된 작업이 없습니다" description="오른쪽 폼에서 새 작업을 추가해 보세요." /></div> : <DataTable columns={["이름", "트리거", "동시성", "시간대·누락 실행", "마지막 실행", "동작"]}>{jobs.map((job) => <tr className="cursor-pointer transition-colors hover:bg-surface" key={job.name} onClick={() => selectJob(job)}><TableCell><div className="font-medium text-foreground">{job.name}</div><div className="text-xs text-muted">upstream {job.dependencies.upstream.length} · downstream {job.dependencies.downstream.length}</div></TableCell><TableCell><span className="font-mono text-xs text-muted">{describeTrigger(job.trigger)}</span></TableCell><TableCell><Badge tone={job.onOverlap === "skip" ? "neutral" : "info"}>{job.onOverlap}</Badge></TableCell><TableCell><span className="font-mono text-xs text-muted">{job.timezone ?? "-"} · {job.misfirePolicy ?? "-"}</span></TableCell><TableCell>{job.lastRun ? <Badge tone={runTone[job.lastRun.state]}>{job.lastRun.state}</Badge> : "-"}</TableCell><TableCell><div className="flex gap-2"><IconButton label="즉시 실행" disabled={pendingJob === job.name} onClick={(event) => { event.stopPropagation(); void runJobAction(job.name, () => client.triggerJob(job.name)); }}><Play aria-hidden="true" size={15} /></IconButton><IconButton label="삭제" disabled={pendingJob === job.name} onClick={(event) => { event.stopPropagation(); void runJobAction(job.name, () => client.removeJob(job.name, true)); }}><Trash2 aria-hidden="true" size={15} /></IconButton></div></TableCell></tr>)}</DataTable>}
        {nextCursor ? <div className="border-t border-border p-3"><Button onClick={() => void loadNextPage()}>다음 50개</Button></div> : null}
      </Panel>
      <Panel><PanelHeader description={selectedJob ? `${selectedJob.name}의 최근 ${RUN_HISTORY_LIMIT}개 이력입니다.` : "목록에서 작업 하나를 선택하면 bounded 이력을 조회합니다."} title="실행 이력" />
        {runsError ? <div className="p-4"><EmptyState title="실행 이력을 불러오지 못했습니다" description={runsError} /></div> : !selectedJob ? <div className="p-4"><EmptyState title="작업을 선택하세요" description="다른 작업의 이력을 병렬로 요청하지 않습니다." /></div> : jobRuns.length === 0 ? <div className="p-4"><EmptyState title="실행 이력이 없습니다" description="작업을 실행하면 여기에 표시됩니다." /></div> : <DataTable columns={["Run", "상태", "트리거", "예정", "종료", "취소"]}>{jobRuns.map((run) => <tr key={run.runId}><TableCell><span className="font-mono text-xs text-muted">{run.runId}</span></TableCell><TableCell><Badge tone={runTone[run.state]}>{run.state}</Badge></TableCell><TableCell>{run.triggeredBy}</TableCell><TableCell>{run.scheduledAt}</TableCell><TableCell>{run.endedAt ?? "-"}</TableCell><TableCell>{run.state === "pending" || run.state === "running" ? <IconButton label="실행 취소" disabled={pendingJob === selectedJob.name} onClick={() => void runJobAction(selectedJob.name, () => client.cancelRun(selectedJob.name, run.runId))}><X aria-hidden="true" size={15} /></IconButton> : "-"}</TableCell></tr>)}</DataTable>}
      </Panel>
    </div>
    <aside className="grid content-start gap-5"><Panel><PanelHeader title={selectedJob ? "작업 수정" : "작업 추가"} description="backend가 지원하는 트리거와 overlap 정책만 전송합니다." /><div className="grid gap-3 p-4"><Field label="이름"><input className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground" onChange={(event) => setFormName(event.target.value)} value={formName} /></Field><Field label="명령"><input className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground" onChange={(event) => setFormCommand(event.target.value)} value={formCommand} /></Field><label className="grid gap-1 text-xs font-medium text-muted"><span>트리거</span><select className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground" onChange={(event) => setTriggerType(event.target.value as JobTrigger["type"])} value={triggerType}><option value="cron">cron</option><option value="interval">interval</option><option value="one_shot">one-shot</option><option value="depends_on">depends_on</option></select></label><Field label={triggerType === "interval" ? "초" : triggerType === "depends_on" ? "의존 작업(쉼표 구분)" : "값"}><input className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground" onChange={(event) => setTriggerValue(event.target.value)} value={triggerValue} /></Field><label className="grid gap-1 text-xs font-medium text-muted"><span>동시성 정책</span><select className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground" onChange={(event) => setFormOverlap(event.target.value as "skip" | "queue" | "parallel")} value={formOverlap}><option value="skip">skip</option><option value="queue">queue</option><option value="parallel">parallel</option></select></label><Button variant="primary" disabled={pendingJob !== null} onClick={() => void handleSaveJob()}>{selectedJob ? <><RotateCcw aria-hidden="true" size={16} />저장</> : "저장"}</Button></div></Panel></aside>
  </div>;
}
