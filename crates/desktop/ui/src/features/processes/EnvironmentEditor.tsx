import {useEffect,useMemo,useRef,useState} from "react";
import {ArrowLeft,ArrowRight,FileUp,Plus,Trash2,Undo2} from "lucide-react";
import {Modal} from "../../components/ui/Modal";
import {Button} from "../../components/ui/primitives";
import {useFormDrafts} from "../../components/ui/FormDrafts";

export function parseEnvironment(content:string):Record<string,string>{
 const result:Record<string,string>=Object.create(null);
 for(const raw of content.replace(/^\uFEFF/,"").split(/\r?\n/)){const match=raw.match(/^\s*(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$/);if(!match){if(raw.trim()&&!raw.trim().startsWith("#"))throw new Error(`환경변수 형식을 확인하세요: ${raw.slice(0,60)}`);continue;}let value=match[2];if(value.startsWith('"')&&value.endsWith('"'))value=value.slice(1,-1).replace(/\\n/g,"\n").replace(/\\"/g,'"').replace(/\\\\/g,"\\");else if(value.startsWith("'")&&value.endsWith("'"))value=value.slice(1,-1);else value=value.replace(/\s+#.*$/,"").trim();if(Object.hasOwn(result,match[1]))throw new Error(`중복 환경변수 이름: ${match[1]}`);result[match[1]]=value;}return result;
}

interface Row {id:string;name:string;value:string}
const valueAt=(value:Record<string,string>,key:string)=>Object.hasOwn(value,key)?value[key]:undefined;
function issue(rows:Row[],row:Row) {
  return !/^[A-Za-z_][A-Za-z0-9_]*$/.test(row.name)?"이름은 영문자 또는 밑줄로 시작하고 영문자·숫자·밑줄만 사용할 수 있습니다."
    :rows.filter(item=>item.name===row.name).length>1?"같은 이름이 이미 있습니다.":"";
}
function Change({value}:{value:string|undefined}) {return value===undefined?<i className="text-muted">없음</i>:<pre>{value}</pre>;}
export function EnvironmentEditor({value,baseline,onChange,scope="main"}:{value:Record<string,string>;baseline:Record<string,string>;onChange:(value:Record<string,string>)=>void;scope?:string}) {
  const store=useFormDrafts();const fieldKey="environment."+scope;const source=JSON.stringify(value);
  const initial=()=>Object.entries(value).map(([name,value])=>({id:crypto.randomUUID(),name,value}));
  const [local,setLocal]=useState<Row[]>(initial);const cached=store?.fields[fieldKey];
  const rows=cached?.source===source?cached.raw as Row[]:store?initial():local;
  const isValid=rows.every(row=>!issue(rows,row));
  const input=useRef<HTMLInputElement>(null);const names=useRef(new Map<string,HTMLInputElement>());const undo=useRef<Row[][]>([]);
  const [pinned,setPinned]=useState<string>();const [search,setSearch]=useState("");const [error,setError]=useState("");const [notice,setNotice]=useState("");
  const [incoming,setIncoming]=useState<{name:string;values:Record<string,string>}|null>(null);
  const [merged,setMerged]=useState<Record<string,string|undefined>>({});const [unresolved,setUnresolved]=useState<string[]>([]);
  useEffect(()=>{store?.set(fieldKey,{raw:rows,source,isValid});},[fieldKey,source,JSON.stringify(rows),isValid]);
  function publish(next:Row[],remember=false) {
    if(remember)undo.current.push(rows);
    setLocal(next);setNotice("");const valid=next.every(row=>!issue(next,row));
    const values=Object.fromEntries(next.map(row=>[row.name,row.value]));
    store?.set(fieldKey,{raw:next,source:valid?JSON.stringify(values):source,isValid:valid});
    if(valid)onChange(values);
  }
  function patch(id:string,next:Partial<Row>){publish(rows.map(row=>row.id===id?{...row,...next}:row));}
  function add(){const row={id:crypto.randomUUID(),name:"",value:""};setPinned(row.id);publish([...rows,row],true);requestAnimationFrame(()=>names.current.get(row.id)?.focus());}
  const visible=rows.filter(row=>row.id===pinned||Boolean(issue(rows,row))||row.name.toLowerCase().includes(search.toLowerCase()));
  function load(name:string,values:Record<string,string>) {
    const all=[...new Set([...Object.keys(baseline),...Object.keys(value),...Object.keys(values)])].sort();
    const next:Record<string,string|undefined>=Object.create(null);const conflicts:string[]=[];
    for(const key of all){const before=valueAt(baseline,key),current=valueAt(value,key),after=valueAt(values,key);if(current===before)next[key]=after;else if(after===before||after===current)next[key]=current;else{next[key]=current;conflicts.push(key);}}
    setMerged(next);setUnresolved(conflicts);setIncoming({name,values});
  }
  function merge(key:string,next:string|undefined){setMerged(previous=>({...previous,[key]:next}));setUnresolved(previous=>previous.filter(item=>item!==key));}
  const counts=useMemo(()=>({
    added:Object.keys(merged).filter(key=>valueAt(value,key)===undefined&&merged[key]!==undefined).length,
    changed:Object.keys(merged).filter(key=>valueAt(value,key)!==undefined&&merged[key]!==undefined&&merged[key]!==valueAt(value,key)).length,
    deleted:Object.keys(merged).filter(key=>valueAt(value,key)!==undefined&&merged[key]===undefined).length,
  }),[merged,value]);
  return <section><div className="argument-heading"><strong>환경변수 <small>{rows.length}</small></strong><div className="flex gap-2">
    <Button disabled={!isValid} onClick={()=>input.current?.click()}><FileUp size={14}/>파일 열기 · 비교</Button><Button onClick={add}><Plus size={14}/>추가</Button>
    <Button disabled={!undo.current.length} onClick={()=>{const previous=undo.current.pop();if(previous)publish(previous);}}><Undo2 size={14}/>삭제 되돌리기</Button>
  </div></div><input type="file" ref={input} hidden accept=".env,.txt,.example,*" onChange={event=>{const file=event.target.files?.[0];if(file)void file.text().then(text=>{try{load(file.name,parseEnvironment(text));setError("");}catch(cause){setError((cause as Error).message);}});event.target.value="";}}/>
    <input className="editor-search" placeholder="환경변수 검색" aria-label="환경변수 검색" value={search} onChange={event=>setSearch(event.target.value)}/>
    <div className="env-list">{visible.map(row=><div key={row.id} className="env-row"><div><input ref={element=>{if(element)names.current.set(row.id,element);else names.current.delete(row.id);}} aria-label="환경변수 이름" aria-invalid={Boolean(issue(rows,row))} value={row.name} onChange={event=>patch(row.id,{name:event.target.value})}/>{issue(rows,row)&&<small role="alert" className="text-danger">{issue(rows,row)}</small>}</div>
      <textarea aria-label={`${row.name||"새 환경변수"} 값`} rows={row.value.includes("\n")?4:1} value={row.value} onChange={event=>patch(row.id,{value:event.target.value})}/>
      <button type="button" aria-label={`${row.name||"새 환경변수"} 삭제`} onClick={()=>publish(rows.filter(item=>item.id!==row.id),true)}><Trash2 size={14}/></button>
    </div>)}</div>{notice&&<p role="status" className="editor-note">{notice}</p>}{error&&<p role="alert" className="text-danger">{error}</p>}
    {incoming&&<Modal title={`환경변수 비교 · ${incoming.name}`} className="env-merge-modal" onClose={()=>setIncoming(null)}>
      <p className="editor-note">추가 {counts.added} · 변경 {counts.changed} · 삭제 {counts.deleted} · 충돌 {unresolved.length}</p>
      <div className="merge-caption"><span>현재 초안</span><span>반영할 초안 · 직접 수정 가능</span><span>불러온 파일</span></div>
      <div className="merge-grid">{Object.keys(merged).map(key=><div key={key} className={`merge-row ${unresolved.includes(key)?"has-conflict":""}`}><div><b>{key}</b><Change value={valueAt(value,key)}/><small>기준: {valueAt(baseline,key)??"없음"}</small><button type="button" aria-label={`${key} 현재 값 채택`} onClick={()=>merge(key,valueAt(value,key))}><ArrowRight size={14}/></button></div>
        <div className="merge-result"><b>{key} {unresolved.includes(key)&&<em>충돌</em>}</b><textarea aria-label={`${key} 최종값`} rows={merged[key]?.includes("\n")?4:1} value={merged[key]??""} disabled={merged[key]===undefined} onChange={event=>merge(key,event.target.value)}/><label><input type="checkbox" checked={merged[key]===undefined} onChange={event=>merge(key,event.target.checked?undefined:"")}/>변수 삭제</label></div>
        <div><button type="button" aria-label={`${key} 파일 값 채택`} onClick={()=>merge(key,valueAt(incoming.values,key))}><ArrowLeft size={14}/></button><b>{key}</b><Change value={valueAt(incoming.values,key)}/><small>{valueAt(incoming.values,key)===valueAt(baseline,key)?"기준과 동일":"기준에서 변경"}</small></div>
      </div>)}</div><footer className="editor-footer"><span>바깥 편집기를 저장해야 실제 설정에 적용됩니다.</span><Button onClick={()=>setIncoming(null)}>취소</Button><Button variant="primary" disabled={unresolved.length>0} onClick={()=>{const entries=Object.entries(merged).filter((entry):entry is [string,string]=>entry[1]!==undefined);const next=entries.map(([name,value])=>({id:rows.find(row=>row.name===name)?.id??crypto.randomUUID(),name,value}));publish(next,true);setIncoming(null);setNotice("환경변수를 초안에 반영했습니다. 설정을 저장해야 적용됩니다.");}}>초안에 반영</Button></footer>
    </Modal>}
  </section>;
}
