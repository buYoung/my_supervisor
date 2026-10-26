import { useEffect, useRef, useState } from "react";
import { ChevronRight } from "lucide-react";
import { Button } from "../../components/ui/primitives";
import { useOperationsClient, toErrorMessage } from "../../services/use-operations";
import type { LogEntry, LogGroup, LogQuery } from "../../services/log-mapping";

export const dateTime = (value: string) => new Date(value).toLocaleString("sv-SE", {
  year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit",
  second: "2-digit", hour12: false, fractionalSecondDigits: 3,
}).replace(",", ".");
const timeOnly = (value: string) => new Date(value).toLocaleTimeString("en-GB", { hour12: false });
export function Level({ value }: { value: string }) { return <span className={`log-level log-level-${value}`}>{value === "unknown" ? "미분류" : value.toUpperCase()}</span>; }

export function LogRow({ entry, onInspect }: { entry: LogEntry; onInspect: (entry: LogEntry) => void }) {
  return <button className="log-single" onClick={() => onInspect(entry)}><span /><time title={dateTime(entry.raw.timestamp)}>{timeOnly(entry.raw.timestamp)}</time><Level value={entry.level} /><span className="log-source">{entry.source}{entry.runId && <small>실행 {entry.runId.slice(0, 8)}{entry.step?` · ${entry.step}`:""}</small>}</span><code className="log-message">{entry.message}</code><ChevronRight size={14} /></button>;
}

export function RepeatGroup({ group, query, onInspect }: { group: LogGroup; query: LogQuery; onInspect: (entry: LogEntry) => void }) {
  const client = useOperationsClient();
  const [isOpen, setIsOpen] = useState(false);
  const [members, setMembers] = useState<LogEntry[]>([]);
  const [nextOffset, setNextOffset] = useState<number>();
  const [error, setError] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const requestRef = useRef<LogQuery>(query);
  requestRef.current = { ...query, source: group.entry.source, runId: group.entry.runId,
    level: group.entry.level, stream: group.entry.raw.stream, message: group.entry.message,
    instance: group.entry.instance, step: group.entry.step, grouping: "none", offset: 0, limit: 50 };
  useEffect(() => {
    if (!isOpen) return;
    let isCancelled = false;
    setIsLoading(true);
    client.queryLogs(requestRef.current).then(result => {
      if (isCancelled) return;
      setMembers(result.records.map(record => record.entry)); setNextOffset(result.nextOffset); setError("");
    }).catch(cause => { if (!isCancelled) setError(toErrorMessage(cause)); })
      .finally(() => { if (!isCancelled) setIsLoading(false); });
    return () => { isCancelled = true; };
  }, [client, isOpen, group.count, group.lastSeen]);
  async function loadMore() {
    if (nextOffset === undefined) return;
    setIsLoading(true);
    try {
      const result = await client.queryLogs({ ...requestRef.current, offset: nextOffset });
      setMembers(previous => [...previous, ...result.records.map(record => record.entry)]); setNextOffset(result.nextOffset);
    } catch (cause) { setError(toErrorMessage(cause)); } finally { setIsLoading(false); }
  }
  return <details className="log-repeat" onToggle={event => setIsOpen(event.currentTarget.open)}>
    <summary><ChevronRight size={14} className="repeat-chevron" /><time title={dateTime(group.lastSeen)}>{timeOnly(group.lastSeen)}</time><Level value={group.entry.level} />
      <span className="log-source" title={group.entry.source}>{group.entry.source}{group.entry.runId && <small>실행 {group.entry.runId.slice(0, 8)}{group.entry.step?` · ${group.entry.step}`:""}</small>}</span>
      <span className="log-message">{group.entry.message}<span className="log-repeat-period">최초 {dateTime(group.firstSeen)} <span>→</span> 최근 {dateTime(group.lastSeen)}</span></span>
      <span className="repeat-count">× {group.count.toLocaleString("ko-KR")}</span>
    </summary><div className="repeat-members"><p>같은 소스·실행·수준·스트림·내용의 로그 {group.count.toLocaleString("ko-KR")}건</p>
      {members.map(entry => <button className="repeat-member" key={entry.id} onClick={() => onInspect(entry)}><time>{dateTime(entry.raw.timestamp)}</time><code>{entry.raw.line}</code><ChevronRight size={14} /></button>)}
      {error && <p role="alert" className="text-danger">{error}</p>}
      {isLoading ? <p role="status">원본 로그를 불러오는 중…</p> : nextOffset !== undefined && <Button onClick={() => void loadMore()}>50건 더 보기</Button>}
    </div></details>;
}
