import {useId, type ReactNode} from "react";
import type {ProcessConfigDto, CheckPolicyInput} from "../../services/wire-types";
import {CommandEditor} from "./CommandEditor";
import type {CommandTextDraft} from "./command-model";
import {NumberField} from "../../components/ui/NumberField";
import {RepeatedField} from "../../components/ui/RepeatedField";
import {useFormDrafts} from "../../components/ui/FormDrafts";

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

type GuardProps = {config: ProcessConfigDto; onChange: (value: Partial<ProcessConfigDto>) => void; commandDrafts?: Record<string,CommandTextDraft>; onCommandDraftChange?: (key:string,draft:CommandTextDraft)=>void;onOpenHealth?:()=>void};

export function RestartGuards({config, onChange}: GuardProps) {
  const {watch, memory} = config;
  return <>
    <PolicyToggle title="파일 변경" description="지정한 파일이나 폴더가 바뀌면 프로세스를 다시 시작합니다." enabled={Boolean(watch)} effect="변경 시 재시작"
      onChange={enabled => onChange({watch: enabled ? {roots: config.cwd ? [config.cwd] : [], recursive: true, exclusions: [], follow_symlinks: false, debounce_ms: 500} : null})}>
      {watch && <>
        <RepeatedField name="watch.roots" label="감시할 절대 경로" isPath isRequired value={watch.roots} validate={path=>path.startsWith("/")?null:"절대 경로를 입력하세요."} onChange={roots=>onChange({watch:{...watch,roots}})}/>
        <RepeatedField name="watch.exclusions" label="제외할 절대 경로" isPath value={watch.exclusions} validate={path=>path.startsWith("/")?null:"절대 경로를 입력하세요."} onChange={exclusions=>onChange({watch:{...watch,exclusions}})}/><p className="editor-note">제외는 경로 접두사로 비교합니다. glob이나 상대 경로는 확장하지 않습니다.</p>
        <label className="editor-field"><span>변경 모으기 (ms)</span><NumberField name="watch.debounce" min={1} value={watch.debounce_ms} onValueChange={value=>onChange({watch:{...watch,debounce_ms:value!}})}/></label>
        <div className="policy-checks"><label className="editor-check"><input type="checkbox" checked={watch.recursive ?? true} onChange={event => onChange({watch: {...watch, recursive: event.target.checked}})}/>하위 폴더 포함</label><label className="editor-check"><input type="checkbox" disabled={!watch.follow_symlinks} checked={watch.follow_symlinks ?? false} onChange={()=>onChange({watch:{...watch,follow_symlinks:false}})}/>심볼릭 링크 따라가기 · 지원하지 않음</label></div>
      </>}
    </PolicyToggle>
    <PolicyToggle title="메모리 상한 초과" description="메모리 사용량이 설정한 상한을 연속으로 넘으면 다시 시작합니다." enabled={Boolean(memory)} effect="초과 시 재시작"
      onChange={enabled => onChange({memory: enabled ? {ceiling_bytes: 536870912, sample_interval_ms: 5000, consecutive_breaches: 3} : null})}>
      {memory && <div className="editor-columns">
        <label className="editor-field"><span>메모리 상한 (MiB)</span><NumberField name="memory.ceiling" min={1} integer={false} value={memory.ceiling_bytes / 1048576} onValueChange={value=>onChange({memory:{...memory,ceiling_bytes:Math.round(value!*1048576)}})}/></label>
        <label className="editor-field"><span>확인 간격 (ms)</span><NumberField name="memory.interval" min={1} value={memory.sample_interval_ms} onValueChange={value=>onChange({memory:{...memory,sample_interval_ms:value!}})}/></label>
        <label className="editor-field"><span>연속 초과 횟수</span><NumberField name="memory.breaches" min={1} value={memory.consecutive_breaches} onValueChange={value=>onChange({memory:{...memory,consecutive_breaches:value!}})}/></label>
      </div>}
    </PolicyToggle>
  </>;
}

export function HealthChecks({config, onChange, commandDrafts, onCommandDraftChange}: GuardProps) {
  return <div className="guard-controls">
    <CheckControl name="liveness" title="응답 확인 (Liveness)" description="프로세스가 실행 중이어도 응답하지 않는 상태를 감지합니다. 연속 실패하면 재시작합니다." effect="실패 시 재시작" value={config.liveness} onChange={value => onChange({liveness: value})} commandDraft={commandDrafts?.liveness} onCommandDraftChange={draft=>onCommandDraftChange?.("liveness",draft)}/>
    <CheckControl name="readiness" title="준비 상태 (Readiness)" description="요청을 받을 준비가 됐는지 확인합니다. 상태 표시와 순차 재시작에 사용하며, 실패만으로 재시작하지 않습니다." effect="상태 판단에 사용" value={config.readiness} onChange={value => onChange({readiness: value})} commandDraft={commandDrafts?.readiness} onCommandDraftChange={draft=>onCommandDraftChange?.("readiness",draft)}/>
  </div>;
}

export function RollingRestartControls({config, onChange,onOpenHealth}: GuardProps) {
  const {rolling} = config;
  return <PolicyToggle title="순차 재시작" description="직접 ‘순차 재시작’을 실행할 때 적용합니다. 자동 재시작 조건과는 별개입니다." enabled={Boolean(rolling)} effect="수동 실행 정책"
    onChange={enabled => onChange({rolling: enabled ? {max_surge: 1, max_unavailable: 0, readiness_timeout_ms: 30000, routability: false} : null})}>
    {rolling && <>
      {!config.readiness&&<p role="alert" className="editor-note">준비 상태 검사가 필요합니다. {onOpenHealth&&<button type="button" onClick={onOpenHealth}>상태 확인 설정 열기</button>}</p>}
      <div className="editor-columns">{([
        ["max_surge", "추가로 실행 가능한 수"],
        ["max_unavailable", "동시에 중지할 수 있는 수"],
        ["readiness_timeout_ms", "준비 상태 대기 (ms)"],
      ] as const).map(([key, label]) => <label className="editor-field" key={key}><span>{label}</span><NumberField name={"rolling."+key} min={key==="max_unavailable"?0:1} max={key==="max_unavailable"?(config.instances??1)-1:undefined} value={rolling[key]} onValueChange={value=>onChange({rolling:{...rolling,[key]:value}})}/></label>)}</div>
      <label className="editor-check"><input type="checkbox" checked={rolling.routability ?? false} onChange={event => onChange({rolling: {...rolling, routability: event.target.checked}})}/>준비 상태 통과 확인 후 교체 진행</label>
    </>}
  </PolicyToggle>;
}

function CheckControl({name,title, description, effect, value, onChange, commandDraft, onCommandDraftChange}: {
  name:string; title: string; description: string; effect: string;
  value: CheckPolicyInput | null | undefined; onChange: (value: CheckPolicyInput | null) => void;
  commandDraft?: CommandTextDraft; onCommandDraftChange?: (draft:CommandTextDraft)=>void;
}) {
  const drafts=useFormDrafts();
  const changeKind=(type:CheckPolicyInput["kind"]["type"])=>{if(!value)return;drafts?.set(`kind.${name}.${value.kind.type}`,{raw:value.kind,source:"",isValid:true});const remembered=drafts?.fields[`kind.${name}.${type}`]?.raw as CheckPolicyInput["kind"]|undefined;const kind=remembered??(type==="http"?{type,url:"http://127.0.0.1:3000/health",expected_status:200}:type==="tcp"?{type,host:"127.0.0.1",port:3000}:{type,command:"/bin/sh",args:["-c","exit 0"]});onChange({...value,kind});};
  const base: CheckPolicyInput = {kind: {type: "http", url: "http://127.0.0.1:3000/health", expected_status: 200}, interval_ms: 5000, timeout_ms: 3000, consecutive_successes: 1, consecutive_failures: 3};
  return <PolicyToggle title={title} description={description} enabled={Boolean(value)} effect={effect} onChange={enabled => onChange(enabled ? base : null)}>
    {value && <>
      {value.timeout_ms>=value.interval_ms&&<p role="alert" className="text-danger">응답 대기는 확인 간격보다 짧아야 합니다.</p>}
      <label className="editor-field"><span>확인 방식</span><select value={value.kind.type} onChange={event=>changeKind(event.target.value as CheckPolicyInput["kind"]["type"])}><option value="http">HTTP 응답</option><option value="tcp">TCP 연결</option><option value="exec">명령 종료 코드</option></select></label>
      {value.kind.type === "http" && <div className="editor-columns"><label className="editor-field"><span>URL</span><input value={value.kind.url} onChange={event => onChange({...value, kind: {...value.kind as Extract<CheckPolicyInput["kind"], {type: "http"}>, url: event.target.value}})}/></label><label className="editor-field"><span>정상 응답 코드</span><NumberField name={name+".http.status"} min={100} max={599} value={value.kind.expected_status} onValueChange={next=>onChange({...value,kind:{...value.kind as Extract<CheckPolicyInput["kind"],{type:"http"}>,expected_status:next!}})}/></label></div>}
      {value.kind.type === "tcp" && <div className="editor-columns"><label className="editor-field"><span>호스트</span><input value={value.kind.host} onChange={event => onChange({...value, kind: {...value.kind as Extract<CheckPolicyInput["kind"], {type: "tcp"}>, host: event.target.value}})}/></label><label className="editor-field"><span>포트</span><NumberField name={name+".tcp.port"} min={1} max={65535} value={value.kind.port} onValueChange={next=>onChange({...value,kind:{...value.kind as Extract<CheckPolicyInput["kind"],{type:"tcp"}>,port:next!}})}/></label></div>}
      {value.kind.type === "exec" && <CommandEditor program={value.kind.command} args={value.kind.args} draft={commandDraft} onDraftChange={onCommandDraftChange} onChange={(command, args) => onChange({...value, kind: {type: "exec", command, args}})}/>}
      <div className="editor-columns">{([
        ["interval_ms", "확인 간격 (ms)"], ["timeout_ms", "응답 대기 (ms)"],
        ["consecutive_successes", "정상 판정 연속 횟수"], ["consecutive_failures", "실패 판정 연속 횟수"],
      ] as const).map(([key, label]) => <label className="editor-field" key={key}><span>{label}</span><NumberField name={name+"."+key} min={1} value={value[key]} onValueChange={next=>onChange({...value,[key]:next})}/></label>)}</div>
    </>}
  </PolicyToggle>;
}
