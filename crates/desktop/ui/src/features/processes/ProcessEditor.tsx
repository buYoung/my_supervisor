import {useRef,useState} from "react";
import {Modal} from "../../components/ui/Modal";
import {Button} from "../../components/ui/primitives";
import {PathField} from "../../components/ui/PathField";
import type {ProcessConfigDto} from "../../services/wire-types";
import {CommandEditor} from "./CommandEditor";
import {EnvironmentEditor} from "./EnvironmentEditor";
import {HealthChecks, RollingRestartControls} from "./GuardControls";
import {RestartControls} from "./RestartControls";
import type {CommandTextDraft} from "./command-model";
import {FormDraftsProvider,useFormDrafts} from "../../components/ui/FormDrafts";
import {NumberField} from "../../components/ui/NumberField";
import {useOperationsClient,toErrorMessage} from "../../services/use-operations";
import {useEditorSession} from "../../shared/editor-session";
import {reconcileDraft,stableJson,type DraftChoices} from "../../shared/reconcile-draft";
import {DraftConflict} from "../../components/ui/DraftConflict";
import {processValidation} from "./process-validation";

export type ProcessEditorSection = "execution" | "environment" | "restart" | "health" | "management";
const sections: Array<{id: ProcessEditorSection; label: string; description: string}> = [
  {id: "execution", label: "실행", description: "프로세스 이름, 실행 명령과 작업 디렉터리를 설정합니다."},
  {id: "environment", label: "환경변수", description: "프로세스에 전달할 환경변수를 편집합니다."},
  {id: "restart", label: "자동 재시작", description: "다시 시작할 조건과 반복 실패 시의 동작을 설정합니다."},
  {id: "health", label: "상태 확인", description: "실행 중인 프로세스의 응답과 준비 상태를 확인합니다."},
  {id: "management", label: "관리·종료", description: "관리 주체, 자동 시작, 실행 수와 종료 방식을 설정합니다."},
];

type ProcessEditorProps = {
  initial: ProcessConfigDto;
  onSave: (config: ProcessConfigDto,expected?:ProcessConfigDto) => Promise<void>;
  onClose: () => void;
  isNew?: boolean;
  initialSection?: ProcessEditorSection;
};
export function ProcessEditor(props:ProcessEditorProps) {return <FormDraftsProvider><ProcessEditorForm {...props}/></FormDraftsProvider>;}
function ProcessEditorForm({initial, onSave, onClose, isNew = false, initialSection = "execution"}:ProcessEditorProps) {
  const fieldDrafts=useFormDrafts();const client=useOperationsClient();
  const [baseline,setBaseline]=useState(initial);
  const [conflict,setConflict]=useState<ProcessConfigDto>();const [choices,setChoices]=useState<DraftChoices>({});
  const isSavingRef=useRef(false);
  const [config, setConfig] = useState(() => structuredClone(initial));
  const [section, setSection] = useState<ProcessEditorSection>(initialSection);
  const [error, setError] = useState("");
  const [isSaving, setIsSaving] = useState(false);
  const [commandDrafts, setCommandDrafts] = useState<Record<string, CommandTextDraft>>({});
  const updateCommandDraft = (key: string, draft: CommandTextDraft) => setCommandDrafts(previous => ({...previous, [key]: draft}));
  const hasInvalidCommands = commandDrafts.main?.isValid === false
    || config.liveness?.kind.type === "exec" && commandDrafts.liveness?.isValid === false
    || config.readiness?.kind.type === "exec" && commandDrafts.readiness?.isValid === false;
  const policyCache=useRef<Partial<ProcessConfigDto>>({});
  const patch = (value: Partial<ProcessConfigDto>) => setConfig(previous => {
    const next={...previous,...value};
    for(const key of ["watch","memory","liveness","readiness","rolling"] as const)if(key in value){
      if(previous[key])policyCache.current={...policyCache.current,[key]:previous[key]};
      if(!previous[key]&&value[key]&&policyCache.current[key])Object.assign(next,{[key]:structuredClone(policyCache.current[key])});
    }
    return next;
  });
  const isSystemManaged = config.management_mode?.type === "system_registered";
  const validationErrors=processValidation(config);
  const hasInvalidFields=fieldDrafts?.hasInvalid(key=>{
    for(const policy of ["liveness","readiness"] as const){
      if(key.startsWith(policy+".http."))return config[policy]?.kind.type==="http";
      if(key.startsWith(policy+".tcp."))return config[policy]?.kind.type==="tcp";
    }
    for(const policy of ["watch","memory","liveness","readiness","rolling"] as const)if(key.startsWith(policy+"."))return Boolean(config[policy])&&!isSystemManaged;
    if(key==="process.instances")return !isSystemManaged;
    if(key.startsWith("restart."))return !isSystemManaged&&Boolean((config.restart?.enabled??true)||config.watch||config.memory||config.liveness);
    return true;
  })??false;
  const hasDirectOnlySettings = Boolean(config.watch || config.memory || config.liveness || config.readiness || config.rolling || (config.instances ?? 1) > 1);

  const isDirty=stableJson(config)!==stableJson(baseline)||hasInvalidCommands||hasInvalidFields;
  const session=useEditorSession({label:initial.name||"새 프로세스",isDirty,isValid:!hasInvalidCommands&&!hasInvalidFields&&!validationErrors.length,isSaving,error});
  const close=()=>session.requestClose(onClose);
  const reconciliation=conflict?reconcileDraft(baseline,config,conflict,choices):null;
  const applyReconciliation=(shouldUseSaved=false)=>{if(!conflict)return;const next=shouldUseSaved?conflict:reconciliation!.value;setConfig(next);setBaseline(conflict);setConflict(undefined);setChoices({});if(shouldUseSaved){fieldDrafts?.clear();setCommandDrafts({});}else{const commandState=(value:ProcessConfigDto,key:string)=>key==="main"?[value.command,value.args]:value[key as "liveness"|"readiness"]?.kind;setCommandDrafts(current=>Object.fromEntries(Object.entries(current).filter(([key,draft])=>!draft.isValid||stableJson(commandState(config,key))===stableJson(commandState(next,key)))));}setError("");};
  async function save() {
    if(hasInvalidCommands||hasInvalidFields||validationErrors.length||isSavingRef.current||conflict)return;
    isSavingRef.current=true;setIsSaving(true);setError("");
    const submitted=structuredClone(config);
    try {await onSave(submitted,baseline);if(session.isMounted.current)onClose();}
    catch(cause){if(session.isMounted.current){setError(toErrorMessage(cause));if(!isNew)try{const latest=await client.getProcessConfig(initial.name);if(session.isMounted.current&&stableJson(latest)!==stableJson(baseline)){setConflict(latest);setChoices({});}}catch{ /* Keep the original save failure and local draft. */ }}}
    finally {isSavingRef.current=false;if(session.isMounted.current)setIsSaving(false);}
  }

  return <Modal title={isNew ? "프로세스 등록" : initial.name + " · 설정 편집"} onClose={close} className="process-editor">
    <fieldset disabled={isSaving} className="contents"><div className="editor-tabs process-editor-tabs" aria-label="프로세스 설정 항목">
      {sections.map(item => <button key={item.id} type="button" aria-pressed={section === item.id} onClick={() => setSection(item.id)}>{item.label}</button>)}
    </div>
    <div className="editor-body process-settings-body">
      {section !== "restart" && <p className="settings-intro">{sections.find(item => item.id === section)?.description}</p>}
      {section === "execution" && <>
        <label className="editor-field"><span>프로세스 이름</span><input value={config.name} onChange={event => patch({name: event.target.value})}/></label>
        <CommandEditor program={config.command} args={config.args ?? []} cwd={config.cwd} draft={commandDrafts.main} onDraftChange={draft => updateCommandDraft("main", draft)} onChange={(command, args) => patch({command, args})}/>
        <PathField value={config.cwd ?? ""} onChange={cwd => patch({cwd})}/>
      </>}
      {section === "environment" && <EnvironmentEditor value={config.env ?? {}} baseline={baseline.env ?? {}} onChange={env => patch({env})}/>}
      {section === "restart" && <RestartControls config={config} onChange={patch} onOpenHealth={() => setSection("health")}/>}
      {section === "health" && <>
        <p className="policy-info">프로세스 종료 여부는 별도 설정 없이 감지합니다. 이곳에서는 실행 중인 프로세스가 제대로 응답하는지를 확인합니다.</p>
        {isSystemManaged ? <p className="editor-note">상태 확인은 직접 관리 방식에서 사용할 수 있습니다.</p> : <HealthChecks config={config} onChange={patch} commandDrafts={commandDrafts} onCommandDraftChange={updateCommandDraft}/>}
      </>}
      {section === "management" && <>
        <section className="management-section">
          <div className="settings-section-heading"><h3>관리 방식</h3><p>프로세스를 누가 실행하고 복구할지 선택합니다.</p></div>
          <label className="editor-field"><span>관리 주체</span><select value={config.management_mode?.type ?? "direct"} onChange={event => patch({management_mode: event.target.value === "direct" ? {type: "direct"} : {type: "system_registered", unit_name: "my-supervisor-" + config.name}})}>
            <option value="direct">Supervisor 직접 관리</option><option value="system_registered" disabled={hasDirectOnlySettings}>macOS 시스템 서비스{hasDirectOnlySettings ? " · 직접 관리 전용 설정 해제 필요" : ""}</option>
          </select><small>{isSystemManaged ? "macOS가 프로세스 실행과 종료 후 재시작을 관리합니다." : "Supervisor가 종료 감지, 감시 조건과 재시작 간격을 관리합니다."}</small></label>
          {hasDirectOnlySettings && !isSystemManaged && <p className="editor-note">시스템 서비스로 변경하려면 파일·메모리 감시, 상태 확인, 순차 재시작을 끄고 실행 수를 1로 설정하세요.</p>}
          {isSystemManaged && <label className="editor-field"><span>서비스 이름</span><input value={config.management_mode?.type === "system_registered" ? config.management_mode.unit_name : ""} onChange={event => patch({management_mode: {type: "system_registered", unit_name: event.target.value}})}/></label>}
        </section>
        <section className="management-section">
          <div className="settings-section-heading"><h3>시작과 실행 수</h3></div>
          <label className="editor-check"><input type="checkbox" checked={config.autostart ?? false} onChange={event => patch({autostart: event.target.checked})}/>데몬 시작 시 자동 실행</label>
          <div className="editor-columns"><label className="editor-field"><span>인스턴스 수</span><NumberField name="process.instances" min={1} max={128} disabled={isSystemManaged} value={config.instances ?? 1} onValueChange={instances => patch({instances})}/></label>
          <label className="editor-field"><span>앱 종료 후 프로세스</span><select disabled={isSystemManaged} value={config.lifecycle ?? "tied"} onChange={event => patch({lifecycle: event.target.value as "tied" | "detached"})}><option value="tied">함께 종료</option><option value="detached">계속 실행</option></select></label></div>
        </section>
        <section className="management-section">
          <div className="settings-section-heading"><h3>중지 버튼을 눌렀을 때</h3><p>종료 신호를 보낸 뒤 기다립니다. 제한 시간을 넘기면 강제로 종료하며, 자동 재시작하지 않습니다.</p></div>
          <div className="editor-columns"><label className="editor-field"><span>먼저 보낼 종료 신호</span><select value={config.shutdown?.signal ?? "term"} onChange={event => patch({shutdown: {...config.shutdown, signal: event.target.value as "term" | "int" | "kill"}})}><option value="term">SIGTERM · 정상 종료 요청</option><option value="int">SIGINT · 인터럽트</option><option value="kill">SIGKILL · 즉시 강제 종료</option></select></label>
          <label className="editor-field"><span>강제 종료 전 대기 (ms)</span><NumberField name="process.shutdown" min={0} value={config.shutdown?.grace_period_ms ?? 10000} onValueChange={grace_period_ms => patch({shutdown: {...config.shutdown, grace_period_ms}})}/></label></div>
        </section>
        {!isSystemManaged && <RollingRestartControls config={config} onChange={patch} onOpenHealth={()=>setSection("health")}/>}
      </>}
      {validationErrors.map(message=><p key={message} role="alert" className="workspace-alert">{message}</p>)}
      {conflict&&reconciliation&&<DraftConflict conflicts={reconciliation.conflicts} choices={choices} onChoice={(path,value)=>setChoices(current=>({...current,[path]:value}))} onApply={()=>applyReconciliation()} onUseSaved={()=>applyReconciliation(true)} onCancel={()=>setConflict(undefined)}/>}
      {error && <p role="alert" className="workspace-alert">{error}</p>}
    </div>
    </fieldset><footer className="editor-footer"><span>{(hasInvalidCommands||hasInvalidFields) ? "입력 오류를 수정해 주세요." : isNew ? "이 설정으로 프로세스를 등록합니다." : `${initial.name} · 변경 항목 ${Object.keys(config).filter(key=>stableJson(config[key as keyof ProcessConfigDto])!==stableJson(baseline[key as keyof ProcessConfigDto])).join(", ")||"없음"} · 실행 중이면 적용 시 다시 시작합니다.`}</span><Button onClick={close} disabled={isSaving}>취소</Button><Button variant="primary" onClick={() => void save()} disabled={Boolean(conflict) || isSaving || hasInvalidCommands || hasInvalidFields || Boolean(validationErrors.length) || !config.name.trim() || !config.command.trim()}>{isSaving ? "적용 중…" : isNew?"프로세스 등록":"설정 적용"}</Button></footer>
  </Modal>;
}
