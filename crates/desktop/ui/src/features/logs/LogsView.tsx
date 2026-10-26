import {useDisplayTime} from "../../shared/display-time";
import {dateRangeBoundary} from "../../shared/time";
import { useCallback, useEffect, useMemo, useState } from "react";
import { ArrowDownToLine, ChevronDown, ChevronLeft, ChevronRight, Copy, Layers, List, Pause, Play, RefreshCw, Search, Terminal, Workflow } from "lucide-react";
import { Button, EmptyState } from "../../components/ui/primitives";
import { Modal } from "../../components/ui/Modal";
import { useOperationsClient, usePolledResource, toErrorMessage } from "../../services/use-operations";
import type { LogEntry, LogQuery, LogQueryResult } from "../../services/log-mapping";
import { logEntryToWire } from "../../services/log-mapping";
import type { JobRun } from "../../shared/types";
import { Level, LogRow, RepeatGroup } from "./LogRows";
import "./logs.css";

const PAGE_SIZE = 50;
const LEVELS = ["trace", "debug", "info", "warn", "error", "fatal", "unknown"];
const count = (value: number) => value.toLocaleString("ko-KR");

export function LogsView() {
  const client = useOperationsClient();
  const {dateTime:formatDateTime,timeZone} = useDisplayTime();
  const dateTime = (value: string) => formatDateTime(value, true);
  const initial = useMemo(() => new URLSearchParams(window.location.search), []);
  const [kind, setKind] = useState<"process" | "job">(initial.get("kind") === "job" ? "job" : "process");
  const [source, setSource] = useState(initial.get("source") === "all" ? "" : initial.get("source") ?? "");
  const [runId, setRunId] = useState(initial.get("run") ?? "");
  const [step,setStep] = useState(initial.get("step") ?? "");
  const [runs, setRuns] = useState<JobRun[]>([]);
  const [level, setLevel] = useState(initial.get("step")?"all":initial.get("level")||"attention");
  const [stream, setStream] = useState("");
  const [search, setSearch] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [range, setRange] = useState(initial.get("run")?"all":"7");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [grouping, setGrouping] = useState<"repeat" | "none">(() => localStorage.getItem("logs.grouping") === "none" ? "none" : "repeat");
  const [ascending, setAscending] = useState(false);
  const [isLive, setIsLive] = useState(false);
  const [isWrapped, setIsWrapped] = useState(true);
  const [offset, setOffset] = useState(0);
  const [snapshot, setSnapshot] = useState<string>();
  const [data, setData] = useState<LogQueryResult | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [isExporting, setIsExporting] = useState(false);
  const [inspected, setInspected] = useState<LogEntry | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const [context, setContext] = useState<LogEntry | null>(null);
  const processes = usePolledResource(useCallback(() => client.listProcesses(), [client]), 5000);
  const jobs = usePolledResource(useCallback(() => client.listJobs(), [client]), 5000);
  const sourceNames = (kind === "process" ? processes.data : jobs.data)?.map(item => item.name) ?? [];
  useEffect(() => { const timer = window.setTimeout(() => setDebouncedSearch(search), 250); return () => window.clearTimeout(timer); }, [search]);
  useEffect(() => {
    if (kind !== "job" || !source) { setRuns([]); return; }
    let isCancelled = false;
    client.listRuns(source, 100).then(result => { if (!isCancelled) setRuns(result.runs); }).catch(cause => { if (!isCancelled) setError(toErrorMessage(cause)); });
    return () => { isCancelled = true; };
  }, [client, kind, source]);
  const customRange = useMemo(() => {
    if (range !== "custom") return {};
    try { return {since: from ? dateRangeBoundary(from,timeZone) : undefined, until: to ? dateRangeBoundary(to,timeZone,true) : undefined}; }
    catch (cause) { return {error: toErrorMessage(cause)}; }
  }, [range,from,to,timeZone]);
  const since = useMemo(() => range === "custom" ? customRange.since
    : range === "all" ? undefined : new Date(Date.now() - Number(range) * 86400000).toISOString(), [range, customRange.since]);
  const query = useMemo((): LogQuery => context ? ({
    kind: context.kind, source: context.source, runId: context.runId, timeZone,
    since: new Date(new Date(context.raw.timestamp).getTime() - 120000).toISOString(),
    until: new Date(new Date(context.raw.timestamp).getTime() + 120000).toISOString(),
    level: "all", grouping: "none", ascending: true, offset, limit: PAGE_SIZE,
  }) : ({ kind, source: source || undefined, runId: runId || undefined, timeZone, level, stream: stream || undefined,
    step:step||undefined, search: debouncedSearch || undefined, since, until: range === "custom" ? customRange.until : snapshot,
    grouping, ascending, offset, limit: PAGE_SIZE }), [context, kind, source, runId,step, level, stream, debouncedSearch, since, range, to, snapshot, customRange.until, grouping, ascending, offset, timeZone]);
  useEffect(() => { setOffset(0); setSnapshot(undefined); setData(null); setContext(null); }, [kind, source, runId, step, level, stream, debouncedSearch, since, range, to, grouping, ascending]);
  useEffect(() => { localStorage.setItem("logs.grouping", grouping); }, [grouping]);
  useEffect(() => {
    let isCancelled = false;
    if (customRange.error) {setError(customRange.error);setIsLoading(false);return;}
    setIsLoading(true);
    client.queryLogs(query).then(result => {
      if (!isCancelled) { setData(result); setError(""); }
    }).catch(cause => { if (!isCancelled) setError(toErrorMessage(cause)); })
      .finally(() => { if (!isCancelled) setIsLoading(false); });
    return () => { isCancelled = true; };
  }, [client, query, refreshKey, customRange.error]);
  useEffect(() => {
    if (!isLive || context || offset > 0 || isLoading) return;
    const timer = window.setTimeout(() => setRefreshKey(value => value + 1), 5000);
    return () => window.clearTimeout(timer);
  }, [isLive, context, offset, isLoading, refreshKey]);
  const memberQuery = useMemo(() => ({ ...query, until: data?.snapshotAt ?? query.until }), [query, data?.snapshotAt]);
  async function exportLogs() {
    setIsExporting(true); setNotice("");
    try {
      const chunks: string[] = []; let next: number | undefined = 0; let saved = 0; let hasPartial = false;
      const until = data?.snapshotAt ?? new Date().toISOString();
      while (next !== undefined) {
        const page = await client.queryLogs({ ...query, until, grouping: "none", offset: next, limit: 5000 });
        chunks.push(...page.records.map(record => JSON.stringify(logEntryToWire(record.entry)) + "\n")); saved += page.records.length;
        hasPartial ||= page.partialSources.length > 0; next = page.nextOffset;
      }
      const url = URL.createObjectURL(new Blob(chunks, { type: "application/x-ndjson" }));
      const anchor = document.createElement("a"); anchor.href = url; anchor.download = `${kind}-logs-${new Date().toISOString().slice(0, 10)}.jsonl`; anchor.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 1000);
      setNotice(`${count(saved)}건의 원본을 내보냈습니다.${hasPartial ? " 조회 한도에 도달한 소스가 포함되어 있습니다." : ""}`);
    } catch (cause) { setError(toErrorMessage(cause)); } finally { setIsExporting(false); }
  }
  function changeKind(value: "process" | "job") { setKind(value); setSource(""); setRunId(""); setStep(""); }
  return <div className={`log-workspace ${isWrapped ? "is-wrapped" : ""}`}>
    <div className="log-page-heading"><div><h1>로그</h1><p>실행 기록에서 문제의 원인과 반복 패턴을 확인합니다.</p></div><Button onClick={() => void exportLogs()} disabled={isExporting || !data?.totalLines}><ArrowDownToLine size={16} />{isExporting ? "내보내는 중…" : "원본 내보내기"}</Button></div>
    <div className="log-kind-tabs" role="tablist" aria-label="로그 종류"><button role="tab" aria-selected={kind === "process"} onClick={() => changeKind("process")}><Terminal size={16} />프로세스 로그<span>{processes.data?.length ?? 0}</span></button><button role="tab" aria-selected={kind === "job"} onClick={() => changeKind("job")}><Workflow size={16} />작업 로그<span>{jobs.data?.length ?? 0}</span></button></div>
    <div className="log-workbench"><aside className="log-sources"><h2>{kind === "process" ? "프로세스" : "작업"}<span>{sourceNames.length}</span></h2><button className={!source ? "is-active" : ""} onClick={() => { setSource(""); setRunId(""); setStep(""); }}><Layers size={15} />모든 {kind === "process" ? "프로세스" : "작업"}</button>{sourceNames.map(name => <button key={name} className={source === name ? "is-active" : ""} title={name} onClick={() => { setSource(name); setRunId(""); setStep(""); }}><span className="source-dot" /><span className="truncate">{name}</span></button>)}<div className="log-source-note">stdout · stderr · system<br />원본 스트림과 로그 수준은 별도로 구분합니다.</div></aside>
      <section className="log-main" aria-label="로그 조회 결과">
        <div className="log-filterbar"><label className="log-search"><Search size={16} /><input aria-label="로그 검색" placeholder="메시지·오류·키워드 검색" value={search} onChange={event => setSearch(event.target.value)} /></label><select aria-label="조회 기간" value={range} onChange={event => setRange(event.target.value)}><option value="1">최근 24시간</option><option value="7">최근 7일</option><option value="30">최근 30일</option><option value="210">최근 7개월</option><option value="all">보관된 전체</option><option value="custom">기간 지정</option></select><Button onClick={() => { setOffset(0); setSnapshot(undefined); setRefreshKey(value => value + 1); }} disabled={isLoading} aria-label="로그 새로고침"><RefreshCw size={15} className={isLoading ? "animate-spin" : ""} /></Button></div>
        {range === "custom" && <div className="log-filterbar"><span className="text-muted">{timeZone} 기준</span><label>시작 <input aria-label="시작일" type="date" value={from} onChange={event => setFrom(event.target.value)} /></label><label>종료 <input aria-label="종료일" type="date" value={to} onChange={event => setTo(event.target.value)} /></label></div>}
        {kind === "job" && source && <div className="log-run-filter"><label htmlFor="log-run">실행 기록</label><select id="log-run" value={runId} onChange={event => {setRunId(event.target.value);setStep("");}}><option value="">이 작업의 모든 실행</option>{runs.map(run => <option value={run.runId} key={run.runId}>{dateTime(run.startedAt ?? run.scheduledAt)} · {run.state} · {run.runId.slice(0, 8)}</option>)}</select></div>}
        {kind === "job" && step && <div className="log-context-banner"><span>단계: {step}</span><Button onClick={()=>setStep("")}>모든 단계 보기</Button></div>}
        <div className="log-level-toolbar"><div className="log-presets"><button aria-pressed={level === "attention"} onClick={() => setLevel("attention")}>주의 필요</button><button aria-pressed={level === "all"} onClick={() => setLevel("all")}>전체 수준</button><button aria-pressed={level === "without_info"} onClick={() => setLevel("without_info")}>INFO 제외</button></div><select aria-label="출력 스트림" value={stream} onChange={event => setStream(event.target.value)}><option value="">모든 스트림</option><option>stdout</option><option>stderr</option><option>system</option></select></div>
        <div className="log-level-counts">{LEVELS.map(value => <button key={value} aria-pressed={level === value} onClick={() => setLevel(value)}><Level value={value} /><span>{count(data?.levelCounts[value] ?? 0)}</span></button>)}<p>기본: WARN · ERROR · FATAL · 미분류</p></div>
        {context && <div className="log-context-banner"><span>선택 로그 전후 2분 · 모든 수준</span><Button onClick={() => { setContext(null); setOffset(0); }}>검색 결과로 돌아가기</Button></div>}
        <details className="log-volume"><summary><ChevronDown size={14} />보관 기간별 발생량<span>{count(data?.scannedLines ?? 0)}건 조회</span></summary><div className="log-volume-bars">{Object.entries(data?.dailyCounts ?? {}).slice(-31).map(([day, total]) => <button title={`${day}: ${count(total)}건`} key={day} onClick={() => { setRange("custom"); setFrom(day); setTo(day); }}><i style={{ height: `${Math.max(4, total / Math.max(1, ...Object.values(data?.dailyCounts ?? {})) * 54)}px` }} /><span>{day.slice(5)}</span></button>)}</div></details>
        <div className="log-result-toolbar"><span aria-live="polite">{isLoading ? "조회 중…" : `${count(data?.totalLines ?? 0)}건${grouping === "repeat" && !context ? ` · ${count(data?.totalGroups ?? 0)}개 항목` : ""}`}</span><div><div className="log-view-toggle"><button aria-label="반복 묶기" aria-pressed={grouping === "repeat"} onClick={() => setGrouping("repeat")}><Layers size={14} />반복 묶기</button><button aria-label="개별 로그" aria-pressed={grouping === "none"} onClick={() => setGrouping("none")}><List size={14} />개별 로그</button></div><button aria-pressed={isWrapped} onClick={() => setIsWrapped(value => !value)}>줄바꿈</button><button onClick={() => setAscending(value => !value)}>{ascending ? "오래된 순" : "최신 순"}</button><button className={isLive ? "is-live" : ""} aria-pressed={isLive} onClick={() => { setIsLive(value => !value); setSnapshot(undefined); setOffset(0); }}>{isLive ? <Pause size={13} /> : <Play size={13} />}{isLive ? "실시간" : "실시간 보기"}</button></div></div>
        {(error || (kind === "process" ? processes.errorMessage : jobs.errorMessage)) && <div className="log-alert" role="alert">{error || (kind === "process" ? processes.errorMessage : jobs.errorMessage)}</div>}
        {data?.partialSources.length ? <div className="log-alert" role="status">일부 기록만 조회됨: {data.partialSources.join(", ")} · 조회 한도 또는 소스 제약이 있습니다. 소스와 기간을 좁혀 주세요.</div> : null}{notice && <div className="log-notice" role="status">{notice}</div>}
        <div className="log-lines" aria-busy={isLoading}>{data?.records.map(group => group.count > 1 && grouping === "repeat" && !context ? <RepeatGroup key={JSON.stringify([group.entry.source, group.entry.runId, group.entry.instance, group.entry.step, group.entry.level, group.entry.raw.stream, group.entry.message])} group={group} query={memberQuery} onInspect={setInspected} /> : <LogRow key={group.entry.id} entry={group.entry} onInspect={setInspected} />)}{!isLoading && !data?.records.length && <div className="p-5"><EmptyState title="조건에 맞는 로그가 없습니다" description="기간이나 로그 수준을 조정하세요. 수집된 실제 기록만 표시합니다." /></div>}</div>
        <footer className="log-pagination"><span>{data ? dateTime(data.snapshotAt) : "—"} 기준</span><div><Button disabled={offset === 0} onClick={() => { setSnapshot(data?.snapshotAt); setOffset(value => Math.max(0, value - PAGE_SIZE)); }}><ChevronLeft size={14} />이전</Button><span>{Math.floor(offset / PAGE_SIZE) + 1}</span><Button disabled={data?.nextOffset === undefined} onClick={() => { setSnapshot(data?.snapshotAt); setOffset(data?.nextOffset ?? 0); }}>다음<ChevronRight size={14} /></Button></div></footer>
      </section></div>
    {inspected && <Modal title="로그 원본" onClose={() => setInspected(null)}><div className="log-inspector"><div className="flex items-center gap-3"><Level value={inspected.level} /><strong>{inspected.source}</strong><span>{inspected.raw.stream}</span></div><dl><dt>발생 시간</dt><dd>{dateTime(inspected.raw.timestamp)}</dd><dt>종류</dt><dd>{inspected.kind === "process" ? "프로세스" : "작업"}</dd>{inspected.runId && <><dt>실행 ID</dt><dd>{inspected.runId}</dd></>}<dt>시퀀스</dt><dd>{inspected.raw.sequence}</dd></dl><pre>{(() => { try { return JSON.stringify(JSON.parse(inspected.raw.line), null, 2); } catch { return inspected.raw.line; } })()}</pre><div className="flex flex-wrap justify-end gap-2"><Button onClick={() => { void navigator.clipboard.writeText(inspected.raw.line).then(() => setNotice("원본 로그를 복사했습니다.")).catch(cause => setError(toErrorMessage(cause))); }}><Copy size={14} />복사</Button><Button onClick={() => { setContext(inspected); setOffset(0); setInspected(null); }}>전후 2분 보기</Button></div></div></Modal>}
  </div>;
}
