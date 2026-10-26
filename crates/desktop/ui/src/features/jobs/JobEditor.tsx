import {useEffect,useState} from "react";
import {useDisplayTime} from "../../shared/display-time";
import {toDateTimeInput,fromDateTimeInput} from "../../shared/time";
import {Modal} from "../../components/ui/Modal";
import {Button} from "../../components/ui/primitives";
import {PathField} from "../../components/ui/PathField";
import {CommandEditor} from "../processes/CommandEditor";
import {EnvironmentEditor} from "../processes/EnvironmentEditor";
import {toErrorMessage,useOperationsClient} from "../../services/use-operations";
import type {JobConfigInput} from "../../services/operations-client";
import type {JobTrigger} from "../../shared/types";

export function JobEditor({initial,isNew,onClose,onSave}:{initial:JobConfigInput;isNew:boolean;onClose:()=>void;onSave:(value:JobConfigInput)=>Promise<void>}){
  const client=useOperationsClient();const {timeZone,dateTime}=useDisplayTime();const [value,setValue]=useState({...initial,timezone:initial.timezone||"UTC"});const [busy,setBusy]=useState(false);const [error,setError]=useState("");const [preview,setPreview]=useState<string[]>([]);
  const [runAtInput,setRunAtInput]=useState(()=>initial.trigger.type==="one_shot"?toDateTimeInput(initial.trigger.at,timeZone):"");
  const [timeError,setTimeError]=useState("");
  useEffect(()=>{if(value.trigger.type==="one_shot"){setRunAtInput(toDateTimeInput(value.trigger.at,timeZone));setTimeError("");}},[timeZone,value.trigger.type]);
  const set=(key:string,next:unknown)=>setValue(current=>({...current,[key]:next}));
  const changeTrigger=(type:JobTrigger["type"])=>{setTimeError("");if(type==="one_shot")setRunAtInput(toDateTimeInput(new Date(Date.now()+3600000).toISOString(),timeZone));set("trigger",type==="cron"?{type,expr:"0 2 * * *"}:type==="interval"?{type,everySec:3600}:type==="one_shot"?{type,at:new Date(Date.now()+3600000).toISOString()}:{type,jobs:[]});};
  return <Modal title={isNew?"작업 추가":`${initial.name} · 작업 설정`} className="process-editor" onClose={onClose}><div className="editor-body">
    <label className="editor-field"><span>작업 이름</span><input value={value.name} readOnly={!isNew} onChange={e=>set("name",e.target.value)}/></label>
    <div className="settings-columns"><div><CommandEditor program={value.command} args={value.args||[]} cwd={value.cwd} onChange={(command,args)=>setValue(current=>({...current,command,args}))}/><PathField label="작업 디렉터리" value={value.cwd||""} onChange={cwd=>set("cwd",cwd)}/><details><summary>환경변수</summary><EnvironmentEditor value={value.env||{}} baseline={initial.env||{}} onChange={env=>set("env",env)}/></details></div>
    <div className="schedule-fields"><label className="editor-field"><span>실행 조건</span><select value={value.trigger.type} onChange={e=>changeTrigger(e.target.value as JobTrigger["type"])}><option value="cron">예약 (cron)</option><option value="interval">일정 간격</option><option value="one_shot">한 번 실행</option><option value="depends_on">선행 작업 완료 후</option></select></label>
    {value.trigger.type==="cron"&&<label className="editor-field"><span>cron 표현식 · 분 시 일 월 요일</span><input value={value.trigger.expr} onChange={e=>set("trigger",{type:"cron",expr:e.target.value})}/></label>}
    {value.trigger.type==="interval"&&<label className="editor-field"><span>간격 (초)</span><input type="number" min={1} value={value.trigger.everySec} onChange={e=>set("trigger",{type:"interval",everySec:Number(e.target.value)})}/></label>}
    {value.trigger.type==="one_shot"&&<label className="editor-field"><span>실행 시각 · {timeZone}</span><input type="datetime-local" step={1} value={runAtInput} onChange={e=>{setRunAtInput(e.target.value);try{set("trigger",{type:"one_shot",at:fromDateTimeInput(e.target.value,timeZone)});setTimeError("");}catch(cause){setTimeError(toErrorMessage(cause));}}}/>{timeError&&<small role="alert">{timeError}</small>}</label>}
    {value.trigger.type==="depends_on"&&<label className="editor-field"><span>선행 작업 · 쉼표 구분</span><input value={value.trigger.jobs.join(", ")} onChange={e=>set("trigger",{type:"depends_on",jobs:e.target.value.split(",").map(v=>v.trim()).filter(Boolean)})}/></label>}
    {value.trigger.type==="cron"&&<p className="editor-note">반복 일정 기준: {String(value.timezone||"UTC")} · 결과 표시: {timeZone}<br/>표시 시간대는 설정에서 변경할 수 있으며 기존 반복 일정은 유지됩니다.</p>}
    <label className="editor-field"><span>이미 실행 중이면</span><select value={value.on_overlap||"skip"} onChange={e=>set("on_overlap",e.target.value)}><option value="skip">새 실행 건너뛰기</option><option value="queue">대기열에 넣기</option><option value="parallel">동시에 실행</option></select></label>
    <label className="editor-field"><span>전체 제한 시간 (초)</span><input type="number" min={1} value={value.timeout_sec??3600} onChange={e=>set("timeout_sec",Number(e.target.value))}/></label>
    <Button onClick={()=>void client.previewJob({config:{...value,trigger:value.trigger.type==="interval"?{type:"interval",every_sec:value.trigger.everySec}:value.trigger},at:new Date().toISOString(),count:3}).then(result=>setPreview(result.occurrences.map(item=>item.scheduledAt))).catch(e=>setError(toErrorMessage(e)))}>다음 실행 시각 확인</Button>{preview.map(time=><div className="schedule-preview" key={time}>{dateTime(time)} · {timeZone}</div>)}</div></div>
    <p className="editor-note">여러 단계를 실행할 작업은 저장 후 흐름 편집에서 구성하세요.</p>{error&&<p role="alert" className="text-danger">{error}</p>}
    </div><footer className="editor-footer"><Button onClick={onClose}>취소</Button><Button variant="primary" disabled={busy||!value.name||!value.command||Boolean(timeError)} onClick={()=>{setBusy(true);onSave(value).then(onClose).catch(e=>setError(toErrorMessage(e))).finally(()=>setBusy(false));}}>{busy?"저장 중…":"저장"}</Button></footer></Modal>;
}
