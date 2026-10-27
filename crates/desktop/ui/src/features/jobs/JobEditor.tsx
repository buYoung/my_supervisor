import {useEffect,useMemo,useRef,useState} from "react";
import {useEditorSession} from "../../shared/editor-session";
import {stableJson} from "../../shared/reconcile-draft";
import {useDisplayTime} from "../../shared/display-time";
import {toDateTimeInput,fromDateTimeInput,supportedTimeZones} from "../../shared/time";
import {Modal} from "../../components/ui/Modal";
import {Button} from "../../components/ui/primitives";
import {PathField} from "../../components/ui/PathField";
import {CommandEditor} from "../processes/CommandEditor";
import {FormDraftsProvider,useFormDrafts} from "../../components/ui/FormDrafts";
import {NumberField} from "../../components/ui/NumberField";
import {RepeatedField} from "../../components/ui/RepeatedField";
import type {CommandTextDraft} from "../processes/command-model";
import {EnvironmentEditor} from "../processes/EnvironmentEditor";
import {toErrorMessage,useOperationsClient} from "../../services/use-operations";
import {jobConfigToWire} from "../../services/wire-mapping";
import type {JobConfigInput} from "../../services/operations-client";
import type {JobPreview,JobTrigger} from "../../shared/types";
import {describeCron,schedulePresets} from "./schedule-model";

type JobEditorProps={initial:JobConfigInput;isNew:boolean;onClose:()=>void;onSave:(value:JobConfigInput)=>Promise<void>;jobNames?:string[]};
export function JobEditor(props:JobEditorProps){return <FormDraftsProvider><JobEditorForm {...props}/></FormDraftsProvider>;}
function JobEditorForm({initial,isNew,onClose,onSave,jobNames=[]}:JobEditorProps){
  const fieldDrafts=useFormDrafts();const client=useOperationsClient();const {timeZone,dateTime}=useDisplayTime();
  const [value,setValue]=useState(()=>structuredClone(initial));const [isSaving,setIsSaving]=useState(false);const [error,setError]=useState("");
  const [runAtInput,setRunAtInput]=useState(()=>initial.trigger.type==="one_shot"?toDateTimeInput(initial.trigger.at,timeZone):"");
  const [timeError,setTimeError]=useState("");const [commandDraft,setCommandDraft]=useState<CommandTextDraft>();
  const [timeoutMode,setTimeoutMode]=useState(initial.timeout_sec==null?"default":"explicit");const timeoutDraft=useRef(initial.timeout_sec);
  const [preset,setPreset]=useState("custom");const customCron=useRef(initial.trigger.type==="cron"?initial.trigger.expr:"0 2 * * *");
  const [preview,setPreview]=useState<{key:string;result:JobPreview}>();const [previewState,setPreviewState]=useState<"idle"|"loading"|"current"|"error">("idle");const [previewError,setPreviewError]=useState("");
  const [serverTimeZone,setServerTimeZone]=useState("");const previewRequest=useRef(0);
  const previewKey=stableJson({trigger:value.trigger,timezone:value.timezone});const currentPreviewKey=useRef(previewKey);currentPreviewKey.current=previewKey;
  const timeZones=useMemo(()=>supportedTimeZones(),[]);
  useEffect(()=>{if(value.trigger.type==="one_shot"&&!timeError)setRunAtInput(toDateTimeInput(value.trigger.at,timeZone));},[timeZone,value.trigger.type]);
  useEffect(()=>{previewRequest.current++;setPreview(undefined);setPreviewState("idle");setPreviewError("");},[previewKey]);
  const set=(key:string,next:unknown)=>setValue(current=>({...current,[key]:next}));
  const triggerDrafts=useRef<Partial<Record<JobTrigger["type"],JobTrigger>>>({});
  const hasInvalidFields=fieldDrafts?.hasInvalid(key=>key==="job.timeout"?timeoutMode==="explicit":!key.startsWith("trigger.")||key.startsWith("trigger."+value.trigger.type+"."))??false;
  const isPastOneShot=value.trigger.type==="one_shot"&&Date.parse(value.trigger.at)<=Date.now();
  const isValid=commandDraft?.isValid!==false&&!hasInvalidFields&&!timeError&&!isPastOneShot&&Boolean(value.name&&value.command)&&(timeoutMode!=="explicit"||value.timeout_sec!=null);
  const changeTrigger=(type:JobTrigger["type"])=>{
    triggerDrafts.current[value.trigger.type]=structuredClone(value.trigger);setTimeError("");
    const next=triggerDrafts.current[type]??(type==="cron"?{type,expr:customCron.current}:type==="interval"?{type,everySec:3600}:type==="one_shot"?{type,at:new Date(Date.now()+3600000).toISOString()}:{type,jobs:[]});
    if(next.type==="one_shot")setRunAtInput(toDateTimeInput(next.at,timeZone));set("trigger",next);
  };
  const isSavingRef=useRef(false);
  const session=useEditorSession({label:initial.name||"새 작업",isDirty:stableJson(value)!==stableJson(initial)||commandDraft?.isValid===false||hasInvalidFields||Boolean(timeError),isValid,isSaving,error});
  const close=()=>session.requestClose(onClose);
  async function save(){if(!isValid||isSavingRef.current)return;isSavingRef.current=true;setIsSaving(true);setError("");const submitted=structuredClone(value);try{await onSave(submitted);if(session.isMounted.current)onClose();}catch(cause){if(session.isMounted.current)setError(toErrorMessage(cause));}finally{isSavingRef.current=false;if(session.isMounted.current)setIsSaving(false);}}
  async function loadPreview(){
    if(!isValid)return;const request=++previewRequest.current,key=previewKey;setPreviewState("loading");setPreviewError("");setPreview(undefined);
    try{const result=await client.previewJob({config:jobConfigToWire(value),at:new Date().toISOString(),count:3});if(!session.isMounted.current||request!==previewRequest.current||key!==currentPreviewKey.current)return;setPreview({key,result});setPreviewState("current");if(result.occurrences[0])setServerTimeZone(result.occurrences[0].timezone);}
    catch(cause){if(!session.isMounted.current||request!==previewRequest.current||key!==currentPreviewKey.current)return;const details=(cause as {details?:{message?:string}})?.details;setPreviewError(details?.message||toErrorMessage(cause));setPreviewState("error");}
  }
  return <Modal title={isNew?"작업 추가":`${initial.name} · 작업 설정`} className="process-editor" onClose={close}><fieldset className="contents" disabled={isSaving}><div className="editor-body">
    <label className="editor-field"><span>작업 이름</span><input value={value.name} readOnly={!isNew} onChange={e=>set("name",e.target.value)}/></label>
    <div className="settings-columns"><div><CommandEditor draft={commandDraft} onDraftChange={setCommandDraft} program={value.command} args={value.args||[]} cwd={value.cwd} onChange={(command,args)=>setValue(current=>({...current,command,args}))}/><PathField label="작업 디렉터리" value={value.cwd||""} onChange={cwd=>set("cwd",cwd)}/><details><summary>환경변수</summary><EnvironmentEditor value={value.env||{}} baseline={initial.env||{}} onChange={env=>set("env",env)}/></details></div>
    <div className="schedule-fields"><label className="editor-field"><span>실행 조건</span><select value={value.trigger.type} onChange={e=>changeTrigger(e.target.value as JobTrigger["type"])}><option value="cron">예약 (cron)</option><option value="interval">일정 간격</option><option value="one_shot">한 번 실행</option><option value="depends_on">선행 작업 완료 후</option></select></label>
    {value.trigger.type==="cron"&&<>
      <label className="editor-field"><span>일정 예시</span><select aria-label="일정 예시" value={preset} onChange={e=>{const id=e.target.value;if(preset==="custom"&&value.trigger.type==="cron")customCron.current=value.trigger.expr;setPreset(id);set("trigger",{type:"cron",expr:id==="custom"?customCron.current:schedulePresets.find(item=>item.id===id)!.expression});}}><option value="custom">직접 입력</option>{schedulePresets.map(item=><option key={item.id} value={item.id}>{item.label}</option>)}</select></label>
      <label className="editor-field"><span>cron 표현식 · 분 시 일 월 요일</span><input aria-label="cron 표현식" value={value.trigger.expr} onChange={e=>{customCron.current=e.target.value;setPreset("custom");set("trigger",{type:"cron",expr:e.target.value});}}/><small>{describeCron(value.trigger.expr)}</small></label>
      <label className="editor-field"><span>일정 시간대</span><select aria-label="일정 시간대" value={String(value.timezone??"")} onChange={e=>set("timezone",e.target.value||undefined)}><option value="">서버 기본{serverTimeZone?` · ${serverTimeZone}`:" · 저장·미리보기에서 결정"}</option>{[...new Set([String(value.timezone||"UTC"),...timeZones])].map(zone=><option key={zone} value={zone}>{zone}</option>)}</select></label>
      <p className="editor-note">반복 일정 기준: {String(value.timezone||serverTimeZone||"서버 기본 시간대")} · 결과 표시: {timeZone}<br/>표시 시간대를 바꿔도 반복 일정의 기준은 바뀌지 않습니다.</p>
    </>}
    {value.trigger.type==="interval"&&<label className="editor-field"><span>간격 (초)</span><NumberField name="trigger.interval.seconds" min={1} value={value.trigger.everySec} onValueChange={everySec=>set("trigger",{type:"interval",everySec})}/></label>}
    {value.trigger.type==="one_shot"&&<label className="editor-field"><span>실행 시각 · {timeZone}</span><input type="datetime-local" step={1} value={runAtInput} onChange={e=>{setRunAtInput(e.target.value);try{const at=fromDateTimeInput(e.target.value,timeZone);if(Date.parse(at)<=Date.now())throw new Error("미래의 실행 시각을 입력하세요.");set("trigger",{type:"one_shot",at});setTimeError("");}catch(cause){setTimeError(toErrorMessage(cause));}}}/>{(timeError||isPastOneShot)&&<small role="alert">{timeError||"미래의 실행 시각을 입력하세요."}</small>}</label>}
    {value.trigger.type==="depends_on"&&<RepeatedField name="trigger.depends_on.jobs" label="선행 작업" value={value.trigger.jobs} isRequired suggestions={jobNames} validate={name=>name===value.name?"자기 자신을 선택할 수 없습니다.":!jobNames.includes(name)?"등록된 작업을 선택하세요.":null} onChange={jobs=>set("trigger",{type:"depends_on",jobs})}/>}
    <label className="editor-field"><span>이미 실행 중이면</span><select value={value.on_overlap||"skip"} onChange={e=>set("on_overlap",e.target.value)}><option value="skip">새 실행 건너뛰기</option><option value="queue">대기열에 넣기</option><option value="parallel">동시에 실행</option></select></label>
    <label className="editor-field"><span>전체 제한 시간</span><select aria-label="전체 제한 시간 방식" value={timeoutMode} onChange={e=>{if(value.timeout_sec!=null)timeoutDraft.current=value.timeout_sec;setTimeoutMode(e.target.value);set("timeout_sec",e.target.value==="default"?undefined:timeoutDraft.current);}}><option value="default">서버 기본 한도 사용</option><option value="explicit">초 단위로 직접 지정</option></select></label>
    {timeoutMode==="explicit"?<label className="editor-field"><span>전체 제한 시간 (초)</span><NumberField name="job.timeout" min={1} value={value.timeout_sec} onValueChange={timeout=>{timeoutDraft.current=timeout;set("timeout_sec",timeout);}}/></label>:<p className="editor-note">미지정 값으로 저장합니다. 현재 흐름 실행기의 기본 한도는 최대 24시간이며 무제한이 아닙니다.</p>}
    <Button disabled={!isValid||previewState==="loading"} onClick={()=>void loadPreview()}>{previewState==="loading"?"미리보기 갱신 중…":"다음 실행 시각 확인"}</Button>
    {previewState==="idle"&&<p className="editor-note">현재 입력으로 다음 실행 시각을 확인하세요.</p>}{previewState==="loading"&&<p role="status">현재 일정의 실행 시각을 확인하고 있습니다.</p>}{previewState==="error"&&<p role="alert">일정 또는 시간대를 확인하세요. {previewError}</p>}
    {preview?.key===previewKey&&<div role="status"><p>현재 일정의 미리보기</p>{preview.result.occurrences.length?preview.result.occurrences.map(time=><div className="schedule-preview" key={time.scheduledAt}><time dateTime={time.scheduledAt}>{dateTime(time.scheduledAt)}</time> · 표시 {timeZone}<small>일정 기준 {time.localTime} · {time.timezone}</small></div>):<p>다음 실행 시각이 없습니다.</p>}</div>}
    </div></div><p className="editor-note">여러 단계를 실행할 작업은 저장 후 흐름 편집에서 구성하세요.</p>{error&&<p role="alert" className="text-danger">{error}</p>}
    </div></fieldset><footer className="editor-footer"><Button onClick={close} disabled={isSaving}>취소</Button><Button variant="primary" disabled={isSaving||!isValid} onClick={()=>void save()}>{isSaving?"저장 중…":"저장"}</Button></footer></Modal>;
}
