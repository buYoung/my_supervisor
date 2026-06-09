import { Play, Plus, SquarePen, Trash2 } from "lucide-react";
import { Badge, Button, DataTable, Field, IconButton, Panel, PanelHeader, TableCell } from "../../components/ui/primitives";
import { jobRuns, jobs } from "../../shared/mock-data";
import type { JobRunState, JobTrigger } from "../../shared/types";

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
  return (
    <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_360px]">
      <div className="grid gap-5">
        <Panel>
          <PanelHeader
            action={
              <Button variant="primary">
                <Plus aria-hidden="true" size={16} />
                작업 추가
              </Button>
            }
            description="cron, interval, one-shot, depends_on 트리거를 같은 목록에서 관리합니다."
            title="작업"
          />
          <DataTable columns={["이름", "트리거", "동시성", "마지막 실행", "다음 예정", "성공률", "동작"]}>
            {jobs.map((job) => (
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
                    <IconButton label="즉시 실행">
                      <Play aria-hidden="true" size={15} />
                    </IconButton>
                    <IconButton label="수정">
                      <SquarePen aria-hidden="true" size={15} />
                    </IconButton>
                    <IconButton label="삭제">
                      <Trash2 aria-hidden="true" size={15} />
                    </IconButton>
                  </div>
                </TableCell>
              </tr>
            ))}
          </DataTable>
        </Panel>

        <Panel>
          <PanelHeader description="최근 실행 이력은 JobRun 타입을 기준으로 표시합니다." title="실행 이력" />
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
        </Panel>
      </div>

      <aside className="grid content-start gap-5">
        <Panel>
          <PanelHeader title="작업 폼" description="JobConfig에 대응하는 주요 필드입니다." />
          <div className="grid gap-3 p-4">
            <Field label="이름" value="nightly-backup" />
            <Field label="명령" value="/usr/local/bin/backup" />
            <Field label="트리거" value="0 2 * * *" />
            <Field label="타임아웃" value="3600" />
            <label className="grid gap-1 text-xs font-medium text-muted">
              <span>동시성 정책</span>
              <select className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary">
                <option>skip</option>
                <option>queue</option>
                <option>parallel</option>
              </select>
            </label>
            <Button variant="primary">저장</Button>
          </div>
        </Panel>
      </aside>
    </div>
  );
}
