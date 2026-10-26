import {useId, type ReactNode} from "react";
import type {ProcessConfigDto, CheckPolicyInput} from "../../services/wire-types";
import {CommandEditor} from "./CommandEditor";

export function PolicyToggle({title, description, enabled, onChange, effect, children}: {
  title: string;
  description: string;
  enabled: boolean;
  onChange: (enabled: boolean) => void;
  effect: string;
  children?: ReactNode;
}) {
  const id = useId();
  return <section className="policy-card" data-enabled={enabled}>
    <div className="policy-card-heading">
      <label className="policy-toggle" htmlFor={id}>
        <input id={id} type="checkbox" role="switch" checked={enabled} onChange={event => onChange(event.target.checked)}/>
        <span><strong>{title}</strong><small>{description}</small></span>
      </label>
      <span className="policy-effect">{enabled ? effect : "사용 안 함"}</span>
    </div>
    {enabled && children && <div className="policy-card-body">{children}</div>}
  </section>;
}

type GuardProps = {config: ProcessConfigDto; onChange: (value: Partial<ProcessConfigDto>) => void};

export function RestartGuards({config, onChange}: GuardProps) {
  const {watch, memory} = config;
  return <>
    <PolicyToggle title="파일 변경" description="지정한 파일이나 폴더가 바뀌면 프로세스를 다시 시작합니다." enabled={Boolean(watch)} effect="변경 시 재시작"
      onChange={enabled => onChange({watch: enabled ? {roots: config.cwd ? [config.cwd] : [], recursive: true, exclusions: [], follow_symlinks: false, debounce_ms: 500} : null})}>
      {watch && <>
        <label className="editor-field"><span>감시할 절대 경로 · 한 줄에 하나</span><textarea rows={3} value={watch.roots.join("\n")} onChange={event => onChange({watch: {...watch, roots: event.target.value.split("\n")}})}/></label>
        <label className="editor-field"><span>제외할 경로 · 한 줄에 하나</span><textarea rows={2} value={watch.exclusions.join("\n")} onChange={event => onChange({watch: {...watch, exclusions: event.target.value.split("\n").filter(Boolean)}})}/></label>
        <label className="editor-field"><span>변경 모으기 (ms)</span><input type="number" min={0} value={watch.debounce_ms} onChange={event => onChange({watch: {...watch, debounce_ms: Number(event.target.value)}})}/></label>
        <div className="policy-checks"><label className="editor-check"><input type="checkbox" checked={watch.recursive ?? true} onChange={event => onChange({watch: {...watch, recursive: event.target.checked}})}/>하위 폴더 포함</label><label className="editor-check"><input type="checkbox" checked={watch.follow_symlinks ?? false} onChange={event => onChange({watch: {...watch, follow_symlinks: event.target.checked}})}/>심볼릭 링크 따라가기</label></div>
      </>}
    </PolicyToggle>
    <PolicyToggle title="메모리 상한 초과" description="메모리 사용량이 설정한 상한을 연속으로 넘으면 다시 시작합니다." enabled={Boolean(memory)} effect="초과 시 재시작"
      onChange={enabled => onChange({memory: enabled ? {ceiling_bytes: 536870912, sample_interval_ms: 5000, consecutive_breaches: 3} : null})}>
      {memory && <div className="editor-columns">
        <label className="editor-field"><span>메모리 상한 (MiB)</span><input type="number" min={1} value={memory.ceiling_bytes / 1048576} onChange={event => onChange({memory: {...memory, ceiling_bytes: Math.round(Number(event.target.value) * 1048576)}})}/></label>
        <label className="editor-field"><span>확인 간격 (ms)</span><input type="number" min={100} value={memory.sample_interval_ms} onChange={event => onChange({memory: {...memory, sample_interval_ms: Number(event.target.value)}})}/></label>
        <label className="editor-field"><span>연속 초과 횟수</span><input type="number" min={1} value={memory.consecutive_breaches} onChange={event => onChange({memory: {...memory, consecutive_breaches: Number(event.target.value)}})}/></label>
      </div>}
    </PolicyToggle>
  </>;
}

export function HealthChecks({config, onChange}: GuardProps) {
  return <div className="guard-controls">
    <CheckControl title="응답 확인 (Liveness)" description="프로세스가 실행 중이어도 응답하지 않는 상태를 감지합니다. 연속 실패하면 재시작합니다." effect="실패 시 재시작" value={config.liveness} onChange={value => onChange({liveness: value})}/>
    <CheckControl title="준비 상태 (Readiness)" description="요청을 받을 준비가 됐는지 확인합니다. 상태 표시와 순차 재시작에 사용하며, 실패만으로 재시작하지 않습니다." effect="상태 판단에 사용" value={config.readiness} onChange={value => onChange({readiness: value})}/>
  </div>;
}

export function RollingRestartControls({config, onChange}: GuardProps) {
  const {rolling} = config;
  return <PolicyToggle title="순차 재시작" description="직접 ‘순차 재시작’을 실행할 때 적용합니다. 자동 재시작 조건과는 별개입니다." enabled={Boolean(rolling)} effect="수동 실행 정책"
    onChange={enabled => onChange({rolling: enabled ? {max_surge: 1, max_unavailable: 0, readiness_timeout_ms: 30000, routability: false} : null})}>
    {rolling && <>
      <div className="editor-columns">{([
        ["max_surge", "추가로 실행 가능한 수"],
        ["max_unavailable", "동시에 중지할 수 있는 수"],
        ["readiness_timeout_ms", "준비 상태 대기 (ms)"],
      ] as const).map(([key, label]) => <label className="editor-field" key={key}><span>{label}</span><input type="number" min={0} value={rolling[key]} onChange={event => onChange({rolling: {...rolling, [key]: Number(event.target.value)}})}/></label>)}</div>
      <label className="editor-check"><input type="checkbox" checked={rolling.routability ?? false} onChange={event => onChange({rolling: {...rolling, routability: event.target.checked}})}/>준비 상태 통과 확인 후 교체 진행</label>
    </>}
  </PolicyToggle>;
}

function CheckControl({title, description, effect, value, onChange}: {
  title: string; description: string; effect: string;
  value: CheckPolicyInput | null | undefined; onChange: (value: CheckPolicyInput | null) => void;
}) {
  const base: CheckPolicyInput = {kind: {type: "http", url: "http://127.0.0.1:3000/health", expected_status: 200}, interval_ms: 5000, timeout_ms: 3000, consecutive_successes: 1, consecutive_failures: 3};
  return <PolicyToggle title={title} description={description} enabled={Boolean(value)} effect={effect} onChange={enabled => onChange(enabled ? base : null)}>
    {value && <>
      <label className="editor-field"><span>확인 방식</span><select value={value.kind.type} onChange={event => onChange({...value, kind: event.target.value === "http" ? base.kind : event.target.value === "tcp" ? {type: "tcp", host: "127.0.0.1", port: 3000} : {type: "exec", command: "/bin/sh", args: ["-c", "exit 0"]}})}><option value="http">HTTP 응답</option><option value="tcp">TCP 연결</option><option value="exec">명령 종료 코드</option></select></label>
      {value.kind.type === "http" && <div className="editor-columns"><label className="editor-field"><span>URL</span><input value={value.kind.url} onChange={event => onChange({...value, kind: {...value.kind as Extract<CheckPolicyInput["kind"], {type: "http"}>, url: event.target.value}})}/></label><label className="editor-field"><span>정상 응답 코드</span><input type="number" min={100} max={599} value={value.kind.expected_status} onChange={event => onChange({...value, kind: {...value.kind as Extract<CheckPolicyInput["kind"], {type: "http"}>, expected_status: Number(event.target.value)}})}/></label></div>}
      {value.kind.type === "tcp" && <div className="editor-columns"><label className="editor-field"><span>호스트</span><input value={value.kind.host} onChange={event => onChange({...value, kind: {...value.kind as Extract<CheckPolicyInput["kind"], {type: "tcp"}>, host: event.target.value}})}/></label><label className="editor-field"><span>포트</span><input type="number" min={1} max={65535} value={value.kind.port} onChange={event => onChange({...value, kind: {...value.kind as Extract<CheckPolicyInput["kind"], {type: "tcp"}>, port: Number(event.target.value)}})}/></label></div>}
      {value.kind.type === "exec" && <CommandEditor program={value.kind.command} args={value.kind.args} onChange={(command, args) => onChange({...value, kind: {type: "exec", command, args}})}/>}
      <div className="editor-columns">{([
        ["interval_ms", "확인 간격 (ms)"], ["timeout_ms", "응답 대기 (ms)"],
        ["consecutive_successes", "정상 판정 연속 횟수"], ["consecutive_failures", "실패 판정 연속 횟수"],
      ] as const).map(([key, label]) => <label className="editor-field" key={key}><span>{label}</span><input type="number" min={1} value={value[key]} onChange={event => onChange({...value, [key]: Number(event.target.value)})}/></label>)}</div>
    </>}
  </PolicyToggle>;
}
