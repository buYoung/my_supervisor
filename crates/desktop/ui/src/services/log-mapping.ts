import type { LogLineDto } from "./wire-types";

export interface LogQuery {
  kind: "process" | "job";
  source?: string;
  runId?: string;
  level?: string;
  stream?: string;
  search?: string;
  message?: string;
  instance?: string;
  step?: string;
  since?: string;
  until?: string;
  grouping?: "repeat" | "none";
  ascending?: boolean;
  offset?: number;
  limit?: number;
}

export interface LogEntry {
  id: string;
  kind: "process" | "job";
  source: string;
  runId?: string;
  instance?: string;
  step?: string;
  level: string;
  message: string;
  raw: LogLineDto;
}
export interface LogGroup {
  entry: LogEntry;
  count: number;
  firstSeen: string;
  lastSeen: string;
  preview: LogEntry[];
}
export interface LogQueryResult {
  records: LogGroup[];
  totalLines: number;
  totalGroups: number;
  scannedLines: number;
  nextOffset?: number;
  snapshotAt: string;
  levelCounts: Record<string, number>;
  dailyCounts: Record<string, number>;
  partialSources: string[];
}

interface LogEntryDto extends Omit<LogEntry, "runId"> { run_id?: string; }
export interface LogQueryResultDto {
  records: { entry: LogEntryDto; count: number; first_seen: string; last_seen: string; preview: LogEntryDto[] }[];
  total_lines: number;
  total_groups: number;
  scanned_lines: number;
  next_offset?: number;
  snapshot_at: string;
  level_counts: Record<string, number>;
  daily_counts: Record<string, number>;
  partial_sources: string[];
}

export function logQueryToWire(query: LogQuery) {
  const { runId, ...rest } = query;
  return { ...rest, run_id: runId };
}
export function logEntryToWire({ runId, ...entry }: LogEntry) {
  return { ...entry, run_id: runId ?? null };
}
export function mapLogQueryResult(dto: LogQueryResultDto): LogQueryResult {
  const mapEntry = ({ run_id, ...entry }: LogEntryDto): LogEntry => ({ ...entry, runId: run_id });
  return {
    records: dto.records.map(group => ({ entry: mapEntry(group.entry), count: group.count,
      firstSeen: group.first_seen, lastSeen: group.last_seen, preview: group.preview.map(mapEntry) })),
    totalLines: dto.total_lines, totalGroups: dto.total_groups, scannedLines: dto.scanned_lines,
    nextOffset: dto.next_offset ?? undefined, snapshotAt: dto.snapshot_at,
    levelCounts: dto.level_counts, dailyCounts: dto.daily_counts, partialSources: dto.partial_sources,
  };
}
