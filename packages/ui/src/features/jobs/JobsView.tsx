import { Play, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Badge, Button, DataTable, EmptyState, Field, IconButton, Panel, PanelHeader, TableCell } from "../../components/ui/primitives";
import { useOperationsClient, usePolledResource, toErrorMessage } from "../../services/use-operations";
import type { JobConfigDto } from "../../services/wire-types";
import type { JobRun, JobRunState, JobTrigger } from "../../shared/types";

const JOBS_POLL_INTERVAL_MS = 2000;
const RUNS_PER_JOB_LIMIT = 20;
const MERGED_RUNS_LIMIT = 50;

const runTone: Record<JobRunState, "success" | "warning" | "danger" | "info" | "neutral"> = {
  pending: "warning",
  running: "info",
  succeeded: "success",
  failed: "danger",
  cancelled: "neutral",
  skipped: "neutral",
};

function describeTrigger(trigger: JobTrigger) {
  if (trigger.type === "cron") {
    return `cron ${trigger.expr}`;
  }
  if (trigger.type === "interval") {
    return `${trigger.everySec}s interval`;
  }
  if (trigger.type === "one_shot") {
    return trigger.at;
  }
  return `depends on ${trigger.jobs.join(", ")}`;
}

export function JobsView() {
  const client = useOperationsClient();
  const [actionError, setActionError] = useState<string | null>(null);
  const [pendingJob, setPendingJob] = useState<string | null>(null);
  const [formName, setFormName] = useState("nightly-backup");
  const [formCommand, setFormCommand] = useState("/usr/local/bin/backup");
  const [formCron, setFormCron] = useState("0 2 * * *");
  const [formOverlap, setFormOverlap] = useState<"skip" | "queue" | "parallel">("skip");
  const [jobRuns, setJobRuns] = useState<JobRun[]>([]);

  const listJobs = useCallback(() => client.listJobs(), [client]);
  const { data: jobs, isLoading, errorMessage, refresh } = usePolledResource(
    listJobs,
    JOBS_POLL_INTERVAL_MS,
  );

  const jobNames = useMemo(() => (jobs ?? []).map((job) => job.name), [jobs]);

  // Runs are per-job only; merge each job's recent history into one cross-job table,
  // sorted by scheduledAt desc and capped (option (a) per the brief discussion). Depending
  // on `jobs` (a fresh array every poll tick and after refresh()) keeps this panel live.
  useEffect(() => {
    if (jobNames.length === 0) {
      setJobRuns([]);
      return;
    }
    let cancelled = false;
    Promise.all(
      jobNames.map((name) =>
        client
          .listRuns(name, RUNS_PER_JOB_LIMIT)
          .then((result) => result.runs)
          .catch(() => [] as JobRun[]),
      ),
    ).then((perJobRuns) => {
      if (cancelled) {
        return;
      }
      const merged = perJobRuns
        .flat()
        .sort((a, b) => (a.scheduledAt < b.scheduledAt ? 1 : -1))
        .slice(0, MERGED_RUNS_LIMIT);
      setJobRuns(merged);
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client, jobs]);

  const runJobAction = useCallback(
    async (name: string, action: () => Promise<unknown>) => {
      setActionError(null);
      setPendingJob(name);
      try {
        await action();
        await refresh();
      } catch (error) {
        setActionError(toErrorMessage(error));
      } finally {
        setPendingJob(null);
      }
    },
    [refresh],
  );

  const handleAddJob = useCallback(async () => {
    setActionError(null);
    if (!formName.trim() || !formCommand.trim()) {
      setActionError("이름과 명령은 필수 항목입니다.");
      return;
    }
    setPendingJob(formName);
    const config: JobConfigDto = {
      name: formName.trim(),
      command: formCommand.trim(),
      trigger: { type: "cron", expr: formCron.trim() },
      on_overlap: formOverlap,
    };
    try {
      await client.addJob(config);
      await refresh();
    } catch (error) {
      setActionError(toErrorMessage(error));
    } finally {
      setPendingJob(null);
    }
  }, [client, formName, formCommand, formCron, formOverlap, refresh]);

  const jobList = jobs ?? [];

  return (
    <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_360px]">
      <div className="grid gap-5">
        <Panel>
          <PanelHeader
            action={
              <Button variant="primary" disabled={pendingJob !== null} onClick={handleAddJob}>
                <Plus aria-hidden="true" size={16} />
                작업 추가
              </Button>
            }
            description="cron, interval, one-shot, depends_on 트리거를 같은 목록에서 관리합니다."
            title="작업"
          />
          {actionError ? (
            <div className="border-b border-border bg-danger/10 px-4 py-2 text-xs font-medium text-danger">
              {actionError}
            </div>
          ) : null}
          {isLoading ? (
            <div className="px-4 py-6 text-sm text-muted">불러오는 중…</div>
          ) : errorMessage && jobList.length === 0 ? (
            <div className="p-4">
              <EmptyState title="데몬에 연결할 수 없습니다" description={errorMessage} />
            </div>
          ) : jobList.length === 0 ? (
            <div className="p-4">
              <EmptyState
                title="등록된 작업이 없습니다"
                description="우측 폼에서 새 작업을 추가해 보세요."
              />
            </div>
          ) : (
            <DataTable columns={["이름", "트리거", "동시성", "마지막 실행", "다음 예정", "성공률", "동작"]}>
              {jobList.map((job) => (
                <tr className="transition-colors duration-200 hover:bg-surface" key={job.name}>
                  <TableCell>
                    <div className="font-medium text-foreground">{job.name}</div>
                    <div className="text-xs text-muted">
                      upstream {job.dependencies.upstream.length} · downstream {job.dependencies.downstream.length}
                    </div>
                  </TableCell>
                  <TableCell>
                    <span className="font-mono text-xs text-muted">{describeTrigger(job.trigger)}</span>
                  </TableCell>
                  <TableCell>
                    <Badge tone={job.onOverlap === "skip" ? "neutral" : "info"}>{job.onOverlap}</Badge>
                  </TableCell>
                  <TableCell>
                    {job.lastRun ? <Badge tone={runTone[job.lastRun.state]}>{job.lastRun.state}</Badge> : "-"}
                  </TableCell>
                  <TableCell>
                    <span className="font-mono text-xs text-muted">{job.nextRunAt ?? "-"}</span>
                  </TableCell>
                  <TableCell>{job.successRateRecent ? `${Math.round(job.successRateRecent * 100)}%` : "-"}</TableCell>
                  <TableCell>
                    <div className="flex items-center gap-2">
                      <IconButton
                        label="즉시 실행"
                        disabled={pendingJob === job.name}
                        onClick={() => runJobAction(job.name, () => client.triggerJob(job.name))}
                      >
                        <Play aria-hidden="true" size={15} />
                      </IconButton>
                      <IconButton
                        label="삭제"
                        disabled={pendingJob === job.name}
                        onClick={() => runJobAction(job.name, () => client.removeJob(job.name, true))}
                      >
                        <Trash2 aria-hidden="true" size={15} />
                      </IconButton>
                    </div>
                  </TableCell>
                </tr>
              ))}
            </DataTable>
          )}
        </Panel>

        <Panel>
          <PanelHeader description="각 작업의 최근 Run 이력을 합쳐 시각 역순으로 표시합니다." title="실행 이력" />
          {jobRuns.length === 0 ? (
            <div className="p-4">
              <EmptyState title="실행 이력이 없습니다" description="작업을 실행하면 이력이 표시됩니다." />
            </div>
          ) : (
            <DataTable columns={["Run", "작업", "상태", "트리거", "시작", "종료", "종료 코드"]}>
              {jobRuns.map((run) => (
                <tr className="transition-colors duration-200 hover:bg-surface" key={run.runId}>
                  <TableCell>
                    <span className="font-mono text-xs text-muted">{run.runId}</span>
                  </TableCell>
                  <TableCell>{run.jobName}</TableCell>
                  <TableCell>
                    <Badge tone={runTone[run.state]}>{run.state}</Badge>
                  </TableCell>
                  <TableCell>{run.triggeredBy}</TableCell>
                  <TableCell>
                    <span className="font-mono text-xs text-muted">{run.startedAt ?? "-"}</span>
                  </TableCell>
                  <TableCell>
                    <span className="font-mono text-xs text-muted">{run.endedAt ?? "-"}</span>
                  </TableCell>
                  <TableCell>{run.exitCode ?? "-"}</TableCell>
                </tr>
              ))}
            </DataTable>
          )}
        </Panel>
      </div>

      <aside className="grid content-start gap-5">
        <Panel>
          <PanelHeader title="작업 폼" description="JobConfig에 대응하는 주요 필드입니다." />
          <div className="grid gap-3 p-4">
            <Field label="이름">
              <input
                className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                onChange={(event) => setFormName(event.target.value)}
                value={formName}
              />
            </Field>
            <Field label="명령">
              <input
                className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                onChange={(event) => setFormCommand(event.target.value)}
                value={formCommand}
              />
            </Field>
            <Field label="트리거 (cron)">
              <input
                className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                onChange={(event) => setFormCron(event.target.value)}
                value={formCron}
              />
            </Field>
            <label className="grid gap-1 text-xs font-medium text-muted">
              <span>동시성 정책</span>
              <select
                className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                onChange={(event) => setFormOverlap(event.target.value as "skip" | "queue" | "parallel")}
                value={formOverlap}
              >
                <option value="skip">skip</option>
                <option value="queue">queue</option>
                <option value="parallel">parallel</option>
              </select>
            </label>
            <Button variant="primary" disabled={pendingJob !== null} onClick={handleAddJob}>
              저장
            </Button>
          </div>
        </Panel>
      </aside>
    </div>
  );
}
