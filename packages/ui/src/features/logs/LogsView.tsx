import { Filter, Pause, Play, Search } from "lucide-react";
import { Badge, Button, Field, Panel, PanelHeader } from "../../components/ui/primitives";
import { logs } from "../../shared/mock-data";

const streamTone = {
  stdout: "success",
  stderr: "danger",
  system: "info",
} as const;

export function LogsView() {
  return (
    <div className="grid gap-5 xl:grid-cols-[320px_minmax(0,1fr)]">
      <Panel>
        <PanelHeader description="프로세스와 Job Run 로그를 같은 패널에서 고릅니다." title="필터" />
        <div className="grid gap-3 p-4">
          <Field label="검색어">
            <div className="relative">
              <Search
                aria-hidden="true"
                className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-muted"
                size={16}
              />
              <input
                className="h-9 w-full rounded-md border border-border bg-surface pl-9 pr-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                defaultValue="spawn_failed"
              />
            </div>
          </Field>
          <label className="grid gap-1 text-xs font-medium text-muted">
            <span>소스</span>
            <select className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary">
              <option>전체</option>
              <option>api-server</option>
              <option>backup-agent</option>
              <option>cache-warmup</option>
            </select>
          </label>
          <label className="grid gap-1 text-xs font-medium text-muted">
            <span>스트림</span>
            <select className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary">
              <option>전체</option>
              <option>stdout</option>
              <option>stderr</option>
              <option>system</option>
            </select>
          </label>
          <div className="grid grid-cols-2 gap-2">
            <Button variant="primary">
              <Filter aria-hidden="true" size={16} />
              적용
            </Button>
            <Button>
              <Play aria-hidden="true" size={16} />
              follow
            </Button>
          </div>
        </div>
      </Panel>

      <Panel className="min-w-0">
        <PanelHeader
          action={
            <Button>
              <Pause aria-hidden="true" size={16} />
              일시 정지
            </Button>
          }
          description="REST tail과 WebSocket follow 화면을 같은 형태로 표현합니다."
          title="로그 스트림"
        />
        <div className="border-b border-border bg-warning/10 px-4 py-2 text-xs font-medium text-warning">
          초당 라인 상한 초과로 42줄이 생략되었습니다.
        </div>
        <div className="overflow-x-auto">
          <div className="min-w-[720px] divide-y divide-border font-mono text-xs">
            {logs.map((log) => (
              <div className="grid grid-cols-[96px_150px_82px_minmax(0,1fr)] gap-3 px-4 py-3" key={log.id}>
                <span className="text-muted">{log.timestamp}</span>
                <span className="truncate text-foreground">{log.source}</span>
                <span>
                  <Badge tone={streamTone[log.stream]}>{log.stream}</Badge>
                </span>
                <span className="whitespace-pre-wrap break-words text-foreground">{log.line}</span>
              </div>
            ))}
          </div>
        </div>
      </Panel>
    </div>
  );
}
