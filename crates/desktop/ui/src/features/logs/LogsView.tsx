import { Pause, Play, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Badge, Button, EmptyState, Field, Panel, PanelHeader } from "../../components/ui/primitives";
import { useOperationsClient, usePolledResource, toErrorMessage } from "../../services/use-operations";
import type { LogLine } from "../../shared/types";

const PROCESS_LIST_POLL_INTERVAL_MS = 2000;
const TAIL_LINE_COUNT = 200;
const MAX_BUFFERED_LINES = 1000;

const streamTone = {
  stdout: "success",
  stderr: "danger",
  system: "info",
} as const;

type StreamFilter = "all" | "stdout" | "stderr" | "system";

export function LogsView() {
  const client = useOperationsClient();
  const [selectedProcess, setSelectedProcess] = useState<string | null>(null);
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [droppedCount, setDroppedCount] = useState(0);
  const [cursorBoundary, setCursorBoundary] = useState<string | null>(null);
  const [logError, setLogError] = useState<string | null>(null);
  const [isFollowing, setIsFollowing] = useState(true);
  const [searchTerm, setSearchTerm] = useState("");
  const [streamFilter, setStreamFilter] = useState<StreamFilter>("all");
  const logKeyCounterRef = useRef(0);
  // Read inside the stream callback so pausing drops incoming lines without tearing down
  // the WS/event subscription or wiping the already-rendered buffer.
  const isFollowingRef = useRef(isFollowing);
  isFollowingRef.current = isFollowing;

  const listProcesses = useCallback(() => client.listProcesses(), [client]);
  const { data: processes, errorMessage: processesError } = usePolledResource(
    listProcesses,
    PROCESS_LIST_POLL_INTERVAL_MS,
  );

  const processNames = useMemo(() => (processes ?? []).map((process) => process.name), [processes]);

  // Default the selection to the first process once the list arrives.
  useEffect(() => {
    if (selectedProcess === null && processNames.length > 0) {
      setSelectedProcess(processNames[0]);
    }
  }, [processNames, selectedProcess]);

  // Seed the tail once, then follow the selected process. Re-runs only when the selection
  // changes — pausing is handled via isFollowingRef inside onLine, so it neither re-seeds nor
  // tears down the stream. The unsubscribe tears down the previous process's stream.
  useEffect(() => {
    if (!selectedProcess) {
      setLogs([]);
      setDroppedCount(0);
      setCursorBoundary(null);
      return;
    }

    let cancelled = false;
    setLogError(null);
    setDroppedCount(0);
    setCursorBoundary(null);
    logKeyCounterRef.current = 0;

    client
      .processLogsTail(selectedProcess, TAIL_LINE_COUNT)
      .then((tail) => {
        if (cancelled) {
          return;
        }
        // Re-key the seed lines against the view-local monotonic counter so list keys stay unique.
        const seeded = tail.lines.map((line) => ({
          ...line,
          id: `seed-${logKeyCounterRef.current++}`,
        }));
        setLogs(seeded);
        setDroppedCount(tail.droppedCount);
        if (tail.cursorExpired) {
          setCursorBoundary(`이전 cursor가 만료되어 보존된 가장 이른 위치${tail.earliestRetainedSequence !== undefined ? ` (${tail.earliestRetainedSequence})` : ""}부터 다시 표시합니다.`);
        } else if (tail.truncated) {
          setCursorBoundary(`tail은 최근 ${TAIL_LINE_COUNT}줄로 제한됩니다${tail.earliestRetainedSequence !== undefined ? ` (보존 시작 ${tail.earliestRetainedSequence})` : ""}.`);
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setLogError(toErrorMessage(error));
        }
      });

    const unsubscribe = client.followProcessLogs(selectedProcess, {
      onLine: (line) => {
        // Paused: drop new lines but keep the rendered buffer and the live subscription.
        if (!isFollowingRef.current) {
          return;
        }
        setLogs((previous) => {
          const next = [...previous, { ...line, id: `live-${logKeyCounterRef.current++}` }];
          return next.length > MAX_BUFFERED_LINES ? next.slice(-MAX_BUFFERED_LINES) : next;
        });
      },
      onDropped: (count) => {
        setDroppedCount((previous) => previous + count);
      },
      onError: (error) => {
        if (!cancelled) {
          setLogError(error.message);
        }
      },
    });

    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, [client, selectedProcess]);

  const visibleLogs = useMemo(() => {
    const term = searchTerm.trim().toLowerCase();
    return logs.filter((log) => {
      if (streamFilter !== "all" && log.stream !== streamFilter) {
        return false;
      }
      if (term && !log.line.toLowerCase().includes(term)) {
        return false;
      }
      return true;
    });
  }, [logs, searchTerm, streamFilter]);

  return (
    <div className="grid gap-5 xl:grid-cols-[320px_minmax(0,1fr)]">
      <Panel>
        <PanelHeader description="선택한 프로세스의 로그를 실시간으로 따라갑니다." title="필터" />
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
                onChange={(event) => setSearchTerm(event.target.value)}
                value={searchTerm}
              />
            </div>
          </Field>
          <label className="grid gap-1 text-xs font-medium text-muted">
            <span>소스</span>
            <select
              className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
              onChange={(event) => setSelectedProcess(event.target.value)}
              value={selectedProcess ?? ""}
            >
              {processNames.length === 0 ? <option value="">프로세스 없음</option> : null}
              {processNames.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
          </label>
          <label className="grid gap-1 text-xs font-medium text-muted">
            <span>스트림</span>
            <select
              className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
              onChange={(event) => setStreamFilter(event.target.value as StreamFilter)}
              value={streamFilter}
            >
              <option value="all">전체</option>
              <option value="stdout">stdout</option>
              <option value="stderr">stderr</option>
              <option value="system">system</option>
            </select>
          </label>
          <div className="grid gap-2">
            <p className="text-xs text-muted">검색과 스트림 필터는 입력 즉시 적용됩니다.</p>
            <Button onClick={() => setIsFollowing((previous) => !previous)}>
              {isFollowing ? <Pause aria-hidden="true" size={16} /> : <Play aria-hidden="true" size={16} />}
              {isFollowing ? "일시 정지" : "follow"}
            </Button>
          </div>
        </div>
      </Panel>

      <Panel className="min-w-0">
        <PanelHeader
          action={
            <Button onClick={() => setIsFollowing((previous) => !previous)}>
              {isFollowing ? <Pause aria-hidden="true" size={16} /> : <Play aria-hidden="true" size={16} />}
              {isFollowing ? "일시 정지" : "follow"}
            </Button>
          }
          description={selectedProcess ? `${selectedProcess} 로그 스트림` : "로그 스트림"}
          title="로그 스트림"
        />
        {droppedCount > 0 ? (
          <div className="border-b border-border bg-warning/10 px-4 py-2 text-xs font-medium text-warning">
            초당 라인 상한 초과로 {droppedCount}줄이 생략되었습니다.
          </div>
        ) : null}
        {cursorBoundary ? <div className="border-b border-border bg-warning/10 px-4 py-2 text-xs font-medium text-warning">{cursorBoundary}</div> : null}
        {logError ? (
          <div className="border-b border-border bg-danger/10 px-4 py-2 text-xs font-medium text-danger">
            {logError}
          </div>
        ) : null}
        {processNames.length === 0 ? (
          <div className="p-4">
            <EmptyState
              title="프로세스가 없습니다"
              description={processesError ?? "로그를 따라갈 프로세스가 등록되어 있지 않습니다."}
            />
          </div>
        ) : visibleLogs.length === 0 ? (
          <div className="p-4">
            <EmptyState title="표시할 로그가 없습니다" description="새 로그 라인이 도착하면 여기에 표시됩니다." />
          </div>
        ) : (
          <div className="overflow-x-auto">
            <div className="min-w-[720px] divide-y divide-border font-mono text-xs">
              {visibleLogs.map((log) => (
                <div className="grid grid-cols-[150px_82px_minmax(0,1fr)] gap-3 px-4 py-3" key={log.id}>
                  <span className="text-muted">{log.timestamp}</span>
                  <span>
                    <Badge tone={streamTone[log.stream]}>{log.stream}</Badge>
                  </span>
                  <span className="whitespace-pre-wrap break-words text-foreground">{log.line}</span>
                </div>
              ))}
            </div>
          </div>
        )}
      </Panel>
    </div>
  );
}
