import type {JobConfigDto, LogLineDto, ProcessConfigDto, ProcessPageDto, ProcessStatusDto} from "../services/wire-types";
import type {Workflow} from "../services/workspace-types";

export const fixtureTime = "2026-10-26T15:00:00.000Z";

export function processFixture(name: string, command = "/usr/bin/printf", args = ["%s", "native fixture"]): ProcessConfigDto {
  return {name, command, args, env: {}, autostart: false, instances: 1, lifecycle: "tied", management_mode: {type: "direct"}, restart: {enabled: false}};
}

export function jobFixture(name: string): JobConfigDto {
  return {name, command: "/usr/bin/printf", args: ["%s", name], trigger: {type: "one_shot", at: new Date(Date.now() + 86_400_000).toISOString()}, on_overlap: "skip"};
}

export function workflowFixture(): Workflow {
  return {version: 1, nodes: [
    {id: "native-trigger", position: {x: 0, y: 0}, data: {kind: "trigger", label: "시작"}},
    {id: "native-command", position: {x: 250, y: 0}, data: {kind: "command", label: "고정 출력", program: "/usr/bin/printf", args: ["%s", "native workflow"]}},
    {id: "native-finish", position: {x: 500, y: 0}, data: {kind: "finish", label: "완료", result: "succeeded"}},
  ], edges: [
    {id: "native-edge-1", source: "native-trigger", target: "native-command", sourceHandle: "out"},
    {id: "native-edge-2", source: "native-command", target: "native-finish", sourceHandle: "out"},
  ]};
}

export function logFixtures(count = 10_000): LogLineDto[] {
  return Array.from({length: count}, (_, index) => ({sequence: index + 1,
    timestamp: new Date(Date.parse(fixtureTime) + index * 1000).toISOString(),
    stream: index % 3 === 0 ? "stderr" : "stdout", line: `native-log-${index + 1} 한글, literal`}));
}

export function processStatusFixture(name: string): ProcessStatusDto {
  return {name, state: "stopped", management_mode: {type: "direct"}, pid: null,
    unit_name: null, restart_count: 0, started_at: null, cpu_percent: 0, memory_bytes: 0};
}

export function partialPageFixture(): ProcessPageDto {
  return {processes: [processStatusFixture("native-partial")], next_cursor: "opaque-fixture-cursor",
    high_watermark: "opaque-fixture-watermark", partial: true, failed_partitions: ["native-unavailable"]};
}

export function delayedFixture<T>(value: T, delayMs: number): Promise<T> {
  return new Promise(resolve => window.setTimeout(() => resolve(value), delayMs));
}

export function failureFixture(message = "native controlled rejection"): Promise<never> {
  return Promise.reject(new Error(message));
}

export const scaleFixtures = {
  jobs: () => Array.from({length: 250}, (_, index) => jobFixture(`native-job-${String(index + 1).padStart(3, "0")}`)),
  process: () => ({...processFixture("native-instances", "/bin/sleep", ["60"]), instances: 50}),
};
