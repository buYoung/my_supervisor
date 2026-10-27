import {useEffect,useId,useLayoutEffect,useRef,useState,type Dispatch,type SetStateAction} from "react";
import {Modal} from "../components/ui/Modal";
import {Button} from "../components/ui/primitives";

interface EditorState {label:string;isDirty:boolean;isValid:boolean;isSaving:boolean;error?:string}
interface PendingLeave {action:()=>void;cancel?:()=>void;labels:string[];ids:string[];isSaving:boolean}
const sessions=new Map<string,()=>EditorState>();
const listeners=new Set<()=>void>();
let pending:PendingLeave|null=null;
function publish(){for(const listener of listeners)listener();}
function refreshPending(){
  if(!pending?.isSaving)return;
  const active=pending.ids.flatMap(id=>{const read=sessions.get(id);return read?[read()]:[];});
  if(active.some(state=>state.isSaving))return;
  pending=active.some(state=>state.isDirty)?{...pending,isSaving:false}:null;publish();
}
export function requestEditorLeave(action:()=>void,only?:string[],cancel?:()=>void){
  const active=[...sessions].filter(([id])=>!only||only.includes(id));
  const affected=active.map(([id,read])=>({id,...read()})).filter(state=>state.isDirty||state.isSaving);
  if(!affected.length){action();return;}
  if(pending)return;
  pending={action,cancel,labels:affected.map(state=>state.label),ids:affected.map(state=>state.id),isSaving:affected.some(state=>state.isSaving)};publish();
}
export function useEditorSession(state:EditorState){
  const id=useId();const latest=useRef(state);latest.current=state;
  const isMounted=useRef(true);
  useLayoutEffect(()=>{isMounted.current=true;sessions.set(id,()=>latest.current);return()=>{isMounted.current=false;sessions.delete(id);refreshPending();};},[id]);
  useLayoutEffect(()=>{refreshPending();},[state.isDirty,state.isSaving]);
  return {requestClose:(action:()=>void)=>requestEditorLeave(action,[id]),isMounted};
}
export function EditorLeavePrompt(){
  const [value,setValue]=useState(pending);
  useEffect(()=>{const update=()=>setValue(pending);listeners.add(update);return()=>{listeners.delete(update);};},[]);
  useEffect(()=>{const beforeUnload=(event:BeforeUnloadEvent)=>{if([...sessions.values()].some(read=>read().isDirty||read().isSaving)){event.preventDefault();event.returnValue="";}};window.addEventListener("beforeunload",beforeUnload);return()=>window.removeEventListener("beforeunload",beforeUnload);},[]);
  if(!value)return null;
  const continueEditing=()=>{pending=null;publish();value.cancel?.();};
  return <Modal title={value.isSaving?"저장 완료를 기다려 주세요":"저장하지 않은 변경"} className="editor-leave-prompt" onClose={continueEditing}><div className="editor-body"><p>{value.labels.join(", ")}</p><p>{value.isSaving?"저장 요청이 진행 중입니다. 완료 또는 오류를 확인한 뒤 이동할 수 있습니다.":"변경을 버리고 이동할까요? 계속 편집하면 현재 입력을 유지합니다."}</p></div><footer className="editor-footer"><Button onClick={continueEditing}>계속 편집</Button>{!value.isSaving&&<Button variant="danger" onClick={()=>{pending=null;publish();value.action();}}>변경 버리기</Button>}</footer></Modal>;
}
const remembered=new Map<string,unknown>();
/** In-memory return context only; no drafts or secrets are written to storage. */
export function useSessionValue<T>(key:string,initial:T|(()=>T),override?:T):[T,Dispatch<SetStateAction<T>>]{
  const [value,setValue]=useState<T>(()=>override!==undefined?override:remembered.has(key)?remembered.get(key) as T:typeof initial==="function"?(initial as ()=>T)():initial);
  useEffect(()=>{remembered.set(key,value);},[key,value]);
  const update:Dispatch<SetStateAction<T>>=next=>setValue(current=>{const value=typeof next==="function"?(next as (current:T)=>T)(current):next;remembered.set(key,value);return value;});
  return [value,update];
}

/** Record in-page selection changes for guarded browser Back navigation. */
export function replaceEditorLocation(url:URL){history.replaceState(null,"",url);window.dispatchEvent(new Event("msv:editor-location"));}
