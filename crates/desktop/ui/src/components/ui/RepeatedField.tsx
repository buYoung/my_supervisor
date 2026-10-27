import {useEffect,useRef,useState} from "react";
import {Plus,Trash2,Undo2,FolderSearch} from "lucide-react";
import {Button} from "./primitives";
import {Modal} from "./Modal";
import {choosePath} from "./PathField";
import {useFormDrafts} from "./FormDrafts";

interface Row {id:string;text:string}
export function RepeatedField({name,label,value,onChange,suggestions,validate,isRequired=false,isPath=false}:{
  name:string;label:string;value:string[];onChange:(value:string[])=>void;
  suggestions?:string[];validate?:(value:string)=>string|null;isRequired?:boolean;isPath?:boolean;
}) {
  const store=useFormDrafts();const source=JSON.stringify(value);
  const initial=()=>[...value.map(text=>({id:crypto.randomUUID(),text})),{id:crypto.randomUUID(),text:""}];
  const [local,setLocal]=useState<Row[]>(initial);
  const cached=store?.fields[name];
  const rows=cached?.source===source?cached.raw as Row[]:store?initial():local;
  const inputs=useRef(new Map<string,HTMLInputElement>());const history=useRef<Row[][]>([]);
  const compositionConfirmation=useRef(false);
  const hasNativeCompositionCommit=useRef(false);
  const nativeCommitClearFrame=useRef<number|undefined>(undefined);
  useEffect(()=>()=>{if(nativeCommitClearFrame.current!==undefined)cancelAnimationFrame(nativeCommitClearFrame.current);},[]);
  const [active,setActive]=useState<string>();const [bulk,setBulk]=useState<{index:number;text:string}|null>(null);const [error,setError]=useState("");
  const values=rows.map(row=>row.text);if(values.at(-1)==="")values.pop();
  const errors=rows.map((row,index)=>!row.text?(index<rows.length-1?"빈 항목을 입력하거나 삭제하세요.":"")
    :values.filter(text=>text===row.text).length>1?"같은 항목이 중복됐습니다.":validate?.(row.text)??"");
  const isValid=!errors.some(Boolean)&&(!isRequired||values.length>0);
  useEffect(()=>{store?.set(name,{raw:rows,source,isValid});},[name,source,JSON.stringify(rows),isValid]);
  function publish(next:Row[],remember=false) {
    if(remember)history.current.push(rows);
    setLocal(next);
    const nextValues=next.map(row=>row.text);if(nextValues.at(-1)==="")nextValues.pop();
    const valid=nextValues.every(text=>Boolean(text)&&nextValues.filter(item=>item===text).length===1&&!validate?.(text))&&(!isRequired||nextValues.length>0);
    store?.set(name,{raw:next,source:JSON.stringify(nextValues),isValid:valid});onChange(nextValues);
  }
  function focus(row:Row){requestAnimationFrame(()=>inputs.current.get(row.id)?.focus());}
  function commit(index:number,text=rows[index].text) {
    if(!text||validate?.(text)||rows.some((row,i)=>i!==index&&row.text===text))return;
    const next=rows.map((row,i)=>i===index?{...row,text}:row);
    if(index===next.length-1)next.push({id:crypto.randomUUID(),text:""});
    publish(next,true);focus(next[index+1]);
  }
  const bulkValues=bulk?(isPath?bulk.text.split(/\r?\n/):suggestions?.includes(bulk.text)?[bulk.text]:bulk.text.split(/[,\r\n]/)).map(text=>text.trim()).filter(Boolean):[];
  return <section className="repeated-field"><div className="argument-heading"><strong>{label}</strong><div className="flex gap-2"><Button onClick={()=>{const index=rows.length-1;commit(index);if(!rows[index].text)focus(rows[index]);}}><Plus size={14}/>항목 추가</Button><Button onClick={()=>setBulk({index:rows.length-1,text:""})}>여러 줄 가져오기</Button><Button disabled={!history.current.length} onClick={()=>{const previous=history.current.pop();if(previous)publish(previous);}}><Undo2 size={14}/>되돌리기</Button></div></div>
    {rows.map((row,index)=><div key={row.id} className="repeated-row"><div className="flex gap-2">
      <input ref={element=>{if(element)inputs.current.set(row.id,element);else inputs.current.delete(row.id);}} aria-label={`${label} ${index+1}`} aria-invalid={Boolean(errors[index])} value={row.text} onFocus={()=>setActive(row.id)} onBlur={()=>{hasNativeCompositionCommit.current=false;window.setTimeout(()=>setActive(current=>current===row.id&&document.activeElement!==inputs.current.get(row.id)?undefined:current),100);}} onChange={event=>publish(rows.map((item,i)=>i===index?{...item,text:event.target.value}:item))} onInput={event=>{
        // WebKit can confirm marked text with an unchanged replacement before
        // delivering Enter without composition events. Keep the guard through
        // that input frame; the next distinct key gesture must remain available.
        if((event.nativeEvent as InputEvent).inputType!=="insertReplacementText"||event.currentTarget.value!==row.text)return;
        hasNativeCompositionCommit.current=true;
        if(nativeCommitClearFrame.current!==undefined)cancelAnimationFrame(nativeCommitClearFrame.current);
        nativeCommitClearFrame.current=requestAnimationFrame(()=>{hasNativeCompositionCommit.current=false;});
      }} onCompositionStart={()=>{compositionConfirmation.current=true;}} onCompositionEnd={()=>{compositionConfirmation.current=true;}} onKeyUp={()=>{compositionConfirmation.current=false;hasNativeCompositionCommit.current=false;}} onKeyDown={event=>{
        if(event.nativeEvent.isComposing||event.keyCode===229)return;
        if(event.key==="Enter"&&(compositionConfirmation.current||hasNativeCompositionCommit.current)){event.preventDefault();hasNativeCompositionCommit.current=false;return;}
        if(event.key==="Enter"){event.preventDefault();commit(index);}
        if(event.key===","&&!isPath&&suggestions){
          if(!row.text){event.preventDefault();return;}
          if(suggestions.includes(row.text)&&!suggestions.some(item=>item.startsWith(row.text+","))){event.preventDefault();commit(index);}
        }
      }} onPaste={event=>{const text=event.clipboardData.getData("text");if(text.includes("\n")||!isPath&&text.includes(",")&&!suggestions?.includes(text)){event.preventDefault();setBulk({index,text});}}}/>
      {isPath&&<button type="button" aria-label={`${label} ${index+1} 폴더 선택`} onClick={()=>void choosePath(true,row.text||undefined).then(path=>{if(path)publish(rows.map((item,i)=>i===index?{...item,text:path}:item));}).catch(cause=>setError(String(cause)))}><FolderSearch size={16}/></button>}
      <button type="button" aria-label={`${label} ${index+1} 삭제`} onClick={()=>{const next=rows.filter(item=>item.id!==row.id);if(!next.length||next.at(-1)?.text)next.push({id:crypto.randomUUID(),text:""});publish(next,true);}}><Trash2 size={15}/></button>
    </div>{errors[index]&&<small role="alert" className="text-danger">{errors[index]}</small>}
    {active===row.id&&suggestions&&<div role="listbox" aria-label={`${label} 후보`}>{suggestions.filter(item=>item.includes(row.text)&&!rows.some(other=>other.id!==row.id&&other.text===item)).slice(0,20).map(item=><button type="button" role="option" key={item} onMouseDown={event=>event.preventDefault()} onClick={()=>commit(index,item)}>{item}</button>)}</div>}</div>)}
    {isRequired&&!values.length&&<small role="alert" className="text-danger">항목을 하나 이상 선택하세요.</small>}{error&&<small role="alert">{error}</small>}
    <p className="editor-note">{isPath?"경로는 쉼표와 공백을 포함한 원문으로 취급합니다. Enter로 다음 행에 입력하세요.":"등록된 이름을 선택하거나 Enter로 확정하세요. 이름이 명확할 때 쉼표로 다음 행에 입력할 수 있습니다."}</p>
    {bulk&&<Modal title="가져올 항목 확인" onClose={()=>setBulk(null)}><div className="editor-body"><textarea aria-label="가져올 항목" rows={6} value={bulk.text} onChange={event=>setBulk({...bulk,text:event.target.value})}/><ol>{bulkValues.map((text,index)=><li key={index}>{text}{validate?.(text)&&<small className="text-danger"> · {validate(text)}</small>}</li>)}</ol></div><footer className="editor-footer"><Button onClick={()=>setBulk(null)}>취소</Button><Button disabled={!bulkValues.length||bulkValues.some(text=>Boolean(validate?.(text)))} onClick={()=>{const next=[...rows.slice(0,bulk.index),...bulkValues.map(text=>({id:crypto.randomUUID(),text})),...rows.slice(bulk.index+1)];if(next.at(-1)?.text)next.push({id:crypto.randomUUID(),text:""});publish(next,true);setBulk(null);}}>확인한 항목 추가</Button></footer></Modal>}
  </section>;
}
