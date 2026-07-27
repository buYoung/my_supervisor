import { RefreshCw, ShieldCheck, TimerReset } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Button, Panel, PanelHeader } from "../../components/ui/primitives";
import { useOperationsClient, usePolledResource } from "../../services/use-operations";

const DAEMON_POLL_INTERVAL_MS = 2000;

export function DaemonView() {
  const client = useOperationsClient();
  const fetchDaemonStatus = useCallback(() => client.daemonStatus(), [client]);
  const fetchJobs = useCallback(() => client.listJobsPage(undefined, undefined, 50), [client]);
  const [eventMessages, setEventMessages] = useState<string[]>([]);
  const {
    data: daemonStatus,
    isLoading,
    errorMessage,
    refresh,
  } = usePolledResource(fetchDaemonStatus, DAEMON_POLL_INTERVAL_MS);
  const { data: jobs } = usePolledResource(fetchJobs, DAEMON_POLL_INTERVAL_MS);

  useEffect(() => client.followEvents({
    onEvent: (event) => setEventMessages((previous) => [`${event.timestamp} · ${event.eventType}`, ...previous].slice(0, 20)),
  }), [client]);

  return (
    <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_360px]">
      <div className="grid gap-5">
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <Panel className="p-4">
            <p className="text-xs font-medium text-muted">버전</p>
            <p className="mt-2 font-mono text-xl font-semibold text-foreground">{daemonStatus?.version ?? "—"}</p>
          </Panel>
          <Panel className="p-4">
            <p className="text-xs font-medium text-muted">데몬 PID</p>
            <p className="mt-2 font-mono text-xl font-semibold text-foreground">{daemonStatus?.pid ?? "—"}</p>
          </Panel>
          <Panel className="p-4">
            <p className="text-xs font-medium text-muted">프로세스</p>
            <p className="mt-2 text-xl font-semibold text-foreground">{daemonStatus?.processCount ?? "—"}</p>
          </Panel>
          <Panel className="p-4">
            <p className="text-xs font-medium text-muted">작업</p>
            <p className="mt-2 text-xl font-semibold text-foreground">{jobs?.records.length ?? "—"}</p>
          </Panel>
        </div>

        <Panel>
          <PanelHeader
            description="GET /api/v1/daemon/status 응답을 기준으로 한 상태 패널입니다."
            title="데몬 상태"
          />
          <div className="grid gap-4 p-4 text-sm md:grid-cols-2">
            <div className="rounded-lg border border-border bg-surface p-4">
              <div className="flex items-center gap-2 font-medium text-foreground">
                <ShieldCheck
                  aria-hidden="true"
                  className={errorMessage === null ? "text-success" : "text-danger"}
                  size={18}
                />
                {isLoading ? "상태 확인 중" : errorMessage === null ? "정상 동작" : "상태 확인 실패"}
              </div>
              <p className="mt-2 text-muted">
                {errorMessage ?? "macOS 로컬 런타임이 연결되어 있습니다."}
              </p>
            </div>
            <div className="rounded-lg border border-border bg-surface p-4">
              <div className="flex items-center gap-2 font-medium text-foreground">
                <TimerReset aria-hidden="true" className="text-info" size={18} />
                시작 시각
              </div>
              <p className="mt-2 font-mono text-xs text-muted">{daemonStatus?.startedAt ?? "—"}</p>
            </div>
            <div className="rounded-lg border border-border bg-surface p-4 md:col-span-2">
              <p className="text-xs font-medium text-muted">설정 경로</p>
              <p className="mt-2 break-all font-mono text-xs text-foreground">{daemonStatus?.configPath ?? "—"}</p>
            </div>
            <div className="rounded-lg border border-border bg-surface p-4 md:col-span-2">
              <p className="text-xs font-medium text-muted">로그 경로</p>
              <p className="mt-2 break-all font-mono text-xs text-foreground">{daemonStatus?.logDir ?? "—"}</p>
            </div>
          </div>
        </Panel>
      </div>

      <aside className="grid content-start gap-5">
        <Panel>
          <PanelHeader title="데몬 제어" description="현재 desktop transport에서 지원되는 안전한 조회 동작입니다." />
          <div className="grid gap-3 p-4">
            <Button variant="primary" onClick={() => void refresh()}>
              <RefreshCw aria-hidden="true" size={16} />
              상태 새로고침
            </Button>
          </div>
        </Panel>

        <Panel className="p-4">
          <p className="text-sm font-medium text-foreground">이벤트 스트림</p>
          {eventMessages.length === 0 ? <p className="mt-1 text-xs text-muted">새 이벤트를 기다리고 있습니다.</p> : <ul className="mt-2 grid gap-1 font-mono text-xs text-muted">{eventMessages.map((event) => <li key={event}>{event}</li>)}</ul>}
        </Panel>
      </aside>
    </div>
  );
}
