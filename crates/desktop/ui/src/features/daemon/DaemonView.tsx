import { Power, RefreshCw, ShieldCheck, TimerReset } from "lucide-react";
import { Badge, Button, Panel, PanelHeader } from "../../components/ui/primitives";
import { daemonStatus, jobs, processes } from "../../shared/mock-data";

export function DaemonView() {
  return (
    <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_360px]">
      <div className="grid gap-5">
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <Panel className="p-4">
            <p className="text-xs font-medium text-muted">버전</p>
            <p className="mt-2 font-mono text-xl font-semibold text-foreground">{daemonStatus.version}</p>
          </Panel>
          <Panel className="p-4">
            <p className="text-xs font-medium text-muted">데몬 PID</p>
            <p className="mt-2 font-mono text-xl font-semibold text-foreground">{daemonStatus.pid}</p>
          </Panel>
          <Panel className="p-4">
            <p className="text-xs font-medium text-muted">프로세스</p>
            <p className="mt-2 text-xl font-semibold text-foreground">{processes.length}</p>
          </Panel>
          <Panel className="p-4">
            <p className="text-xs font-medium text-muted">작업</p>
            <p className="mt-2 text-xl font-semibold text-foreground">{jobs.length}</p>
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
                <ShieldCheck aria-hidden="true" className="text-success" size={18} />
                정상 동작
              </div>
              <p className="mt-2 text-muted">로컬 루프백 주소에서 API와 이벤트 스트림을 제공 중입니다.</p>
            </div>
            <div className="rounded-lg border border-border bg-surface p-4">
              <div className="flex items-center gap-2 font-medium text-foreground">
                <TimerReset aria-hidden="true" className="text-info" size={18} />
                시작 시각
              </div>
              <p className="mt-2 font-mono text-xs text-muted">{daemonStatus.startedAt}</p>
            </div>
            <div className="rounded-lg border border-border bg-surface p-4 md:col-span-2">
              <p className="text-xs font-medium text-muted">설정 경로</p>
              <p className="mt-2 break-all font-mono text-xs text-foreground">{daemonStatus.configPath}</p>
            </div>
            <div className="rounded-lg border border-border bg-surface p-4 md:col-span-2">
              <p className="text-xs font-medium text-muted">로그 경로</p>
              <p className="mt-2 break-all font-mono text-xs text-foreground">{daemonStatus.logDir}</p>
            </div>
          </div>
        </Panel>
      </div>

      <aside className="grid content-start gap-5">
        <Panel>
          <PanelHeader title="데몬 제어" description="실제 호출 전 단계의 버튼 목업입니다." />
          <div className="grid gap-3 p-4">
            <Button variant="primary">
              <RefreshCw aria-hidden="true" size={16} />
              설정 리로드
            </Button>
            <Button variant="danger">
              <Power aria-hidden="true" size={16} />
              데몬 종료
            </Button>
          </div>
        </Panel>

        <Panel className="p-4">
          <div className="flex items-center justify-between gap-3">
            <div>
              <p className="text-sm font-medium text-foreground">이벤트 스트림</p>
              <p className="mt-1 text-xs text-muted">/api/v1/events</p>
            </div>
            <Badge tone="success">connected</Badge>
          </div>
        </Panel>
      </aside>
    </div>
  );
}
