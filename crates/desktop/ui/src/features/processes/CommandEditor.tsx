import {useEffect,useMemo,useRef,useState} from "react";
import {ChevronDown,Plus,Trash2,RefreshCw} from "lucide-react";
import {Modal} from "../../components/ui/Modal";
import {PathField,choosePath} from "../../components/ui/PathField";
import {Button} from "../../components/ui/primitives";
import {useOperationsClient,toErrorMessage} from "../../services/use-operations";
import type {Executable,RuntimeRelease,CommandHelp} from "../../services/workspace-types";
import {argumentRows,basename,parseHelp,quoteArgument,rowsToArguments,splitCommand,helpInvocation,type ArgumentRow,type HelpOption,type CommandTextDraft} from "./command-model";

export function CommandEditor({program,args,cwd,onChange,draft,onDraftChange}:{
  program:string;args:string[];cwd?:string;onChange:(program:string,args:string[])=>void;
  draft?:CommandTextDraft;onDraftChange?:(draft:CommandTextDraft)=>void;
}) {
  const client=useOperationsClient();
  const name=basename(program);
  const [choices,setChoices]=useState<Executable[]>([]);
  const [isOpen,setIsOpen]=useState(false);
  const [install,setInstall]=useState(false);
  const [error,setError]=useState("");
  const [parseError,setParseError]=useState(draft?.isValid===false?"따옴표나 이스케이프 문자를 닫아 주세요.":"");
  const [help,setHelp]=useState<HelpOption[]>([]);
  const [rows,setRows]=useState(()=>argumentRows(program,args));
  const [active,setActive]=useState<string>();
  const [completionIndex,setCompletionIndex]=useState(0);
  const [line,setLine]=useState(()=>draft?.text??args.map(quoteArgument).join(" "));
  const [isProbing,setIsProbing]=useState(false);
  const [isProbeCancelled,setIsProbeCancelled]=useState(false);
  const emitted=useRef("");
  const probeGeneration=useRef(0);
  const canonical=JSON.stringify(args);
  const helpContext=JSON.stringify([program,cwd,helpInvocation(program,args)]);

  useEffect(()=>{
    const key=program+canonical;
    if(emitted.current===key){emitted.current="";return;}
    setRows(previous=>argumentRows(program,JSON.parse(canonical)).map((row,index)=>({...row,id:previous[index]?.id||row.id})));
    if(!draft||draft.isValid){setLine(JSON.parse(canonical).map(quoteArgument).join(" "));setParseError("");}
  },[program,canonical]);
  useEffect(()=>{probeGeneration.current++;setHelp([]);return()=>{probeGeneration.current++;};},[helpContext]);

  function publish(nextProgram:string,values:string[],text=values.map(quoteArgument).join(" ")) {
    emitted.current=nextProgram+JSON.stringify(values);
    setLine(text);setParseError("");onDraftChange?.({text,isValid:true});onChange(nextProgram,values);
  }
  function editLine(text:string) {
    setLine(text);
    try {
      const values=splitCommand(text);
      setRows(previous=>argumentRows(program,values).map((row,index)=>({...row,id:previous[index]?.id||row.id})));
      publish(program,values,text);
    } catch(cause) {
      setParseError(toErrorMessage(cause));onDraftChange?.({text,isValid:false});
    }
  }
  function update(next:ArgumentRow[]) {setRows(next);publish(program,rowsToArguments(next));}
  function patch(id:string,change:Partial<ArgumentRow>) {update(rows.map(row=>row.id===id?{...row,...change}:row));}
  function selectProgram(path:string) {
    emitted.current=path+canonical;onChange(path,args);setIsOpen(false);
  }
  async function discover() {
    setError("");
    try {setChoices(await client.developerTool<Executable[]>({operation:"executables",name}));}
    catch(cause){setError(toErrorMessage(cause));}
  }
  async function probe() {
    if(isProbing||parseError||!program)return;
    const generation=++probeGeneration.current;
    setIsProbing(true);setIsProbeCancelled(false);setError("");
    try {
      const result=await client.developerTool<CommandHelp>({operation:"help",program,args:helpInvocation(program,args),cwd});
      if(generation===probeGeneration.current) {
        setHelp(result.supported?parseHelp(result.text,"script"):[]);
        if(!result.supported)setError(result.text||"도움말을 확인하지 못했습니다.");
      }
    } catch(cause){if(generation===probeGeneration.current)setError(toErrorMessage(cause));}
    finally {setIsProbing(false);setIsProbeCancelled(false);}
  }
  const activeRow=rows.find(row=>row.id===active);
  const suggestions=useMemo(()=>help.filter(option=>!activeRow?.name||option.name.toLowerCase().includes(activeRow.name.toLowerCase())).slice(0,30),[help,activeRow?.name]);
  function complete(option:HelpOption) {
    if(!activeRow)return;
    patch(activeRow.id,{name:option.name,separator:option.separator,hasValue:option.requiresValue,value:option.requiresValue?activeRow.value:""});
    setActive(undefined);
  }
  return <div className="command-editor">
    <label className="editor-field"><span>실행 명령 · 실행 파일과 인수</span><div className="command-line">
      <button type="button" className="runtime-chip" onClick={()=>setIsOpen(!isOpen)}>{name||"실행자"}<ChevronDown size={13}/></button>
      <input aria-label="전체 실행 명령 인수" aria-invalid={Boolean(parseError)} value={line} placeholder="실행 파일을 제외한 인수" onChange={event=>editLine(event.target.value)}/>
    </div><small>오른쪽 입력에는 인수만 입력합니다. 실행 파일 이름과 같은 첫 인수도 그대로 전달합니다.</small></label>
    <div className="runtime-location"><button type="button" title={program} onClick={()=>setIsOpen(!isOpen)}>{program||"실행자 위치를 선택하세요"}</button><button type="button" title="실행자 설치" onClick={()=>setInstall(true)}><Plus size={16}/></button></div>
    {isOpen&&<div className="runtime-picker">
      <Button onClick={()=>void discover()}><RefreshCw size={14}/>실행자 검색 · 버전 조회 실행</Button>
      <p className="editor-note">검색하면 PATH의 실행 파일을 실행해 버전을 확인합니다.</p>
      {choices.map(choice=><button className={choice.path===program?"selected":""} key={choice.path} type="button" onClick={()=>selectProgram(choice.path)}><b>{choice.version}{choice.is_default?" · PATH 기본값":""}</b><small>{choice.path}</small></button>)}
      <PathField directory={false} label="직접 실행자 지정" value={program} onChange={path=>{emitted.current=path+canonical;onChange(path,args);}}/>
      <button type="button" onClick={()=>void choosePath(false,program).then(path=>{if(path)selectProgram(path);}).catch(cause=>setError(toErrorMessage(cause)))}>실행 파일 찾기</button>
    </div>}
    <div className="argument-heading"><strong>옵션과 인수 · 표시된 순서대로 전달</strong><Button disabled={Boolean(parseError)} onClick={()=>update([...rows,{id:crypto.randomUUID(),name:"",value:"",separator:" ",scope:"script"}])}><Plus size={14}/>인수 추가</Button><Button disabled={Boolean(parseError)} onClick={()=>update([...rows,{id:crypto.randomUUID(),name:"",value:"",separator:" ",scope:"script",isEmptyArgument:true}])}>빈 인수 추가</Button></div>
    <div className="argument-header"><span>전달 대상</span><span>옵션 / 역할</span><span>값</span><span/></div>
    {rows.map(row=>{const descriptor=help.find(option=>option.name===row.name);return <div className="argument-row" key={row.id}>
      <select disabled={Boolean(parseError)} aria-label="인수 전달 대상" value={row.scope} onChange={event=>patch(row.id,{scope:event.target.value as ArgumentRow["scope"]})}><option value="runtime">실행자 옵션</option><option value="entry">실행 파일</option><option value="script">스크립트 인수</option></select>
      <div className="argument-name"><input disabled={Boolean(parseError)} aria-label="옵션 이름" aria-expanded={active===row.id&&suggestions.length>0} title={descriptor?.description} value={row.name} placeholder={row.scope==="entry"?"실행 파일":"직접 입력 / 옵션 검색"} onFocus={()=>{setActive(row.id);setCompletionIndex(0);}} onBlur={event=>{const input=event.currentTarget;window.setTimeout(()=>setActive(current=>current===row.id&&document.activeElement!==input?undefined:current),150);}} onChange={event=>patch(row.id,{name:event.target.value})} onKeyDown={event=>{
        if(event.nativeEvent.isComposing||!suggestions.length)return;
        if(event.key==="ArrowDown"||event.key==="ArrowUp"){event.preventDefault();setCompletionIndex(index=>(index+(event.key==="ArrowDown"?1:suggestions.length-1))%suggestions.length);}
        else if(event.key==="Enter"&&active===row.id){event.preventDefault();complete(suggestions[completionIndex%suggestions.length]);}
        else if(event.key==="Escape")setActive(undefined);
      }}/>{active===row.id&&suggestions.length>0&&<div className="option-completions" role="listbox">{suggestions.map((option,index)=><button type="button" role="option" aria-selected={index===completionIndex} key={option.name} onMouseDown={event=>event.preventDefault()} onClick={()=>complete(option)}><strong>{option.name}<em>{option.requiresValue?"값 필요":"값 없음"}</em></strong><small>{option.description}</small></button>)}</div>}</div>
      <input disabled={Boolean(parseError)} aria-label="옵션 값" value={row.value} title={row.isEmptyArgument?"빈 문자열 인수":descriptor?.description} placeholder={row.isEmptyArgument?"빈 문자열":"값 없음"} onChange={event=>patch(row.id,{value:event.target.value,hasValue:row.hasValue||Boolean(event.target.value)})}/>
      <button type="button" disabled={Boolean(parseError)} title="인수 삭제" onClick={()=>update(rows.filter(item=>item.id!==row.id))}><Trash2 size={14}/></button>
    </div>;})}
    <p className="editor-note" aria-label="최종 실행 명령">{[program,...args].map(quoteArgument).join(" ")}</p>
    <Button disabled={isProbing||Boolean(parseError)||!program} onClick={()=>void probe()}>{isProbing?"도움말 조회 중…":"도움말 실행"}</Button>
    {isProbing&&<Button disabled={isProbeCancelled} onClick={()=>{probeGeneration.current++;setIsProbeCancelled(true);}}>결과 받기 취소</Button>}
    <p className="editor-note">도움말 실행은 선택한 실행 파일이나 스크립트를 실제 실행합니다. 각 조회는 최대 12초 안에 종료됩니다.{isProbeCancelled?" 결과를 폐기하고 실행 종료를 기다립니다.":""}</p>
    {parseError&&<p role="alert" className="text-danger text-xs">{parseError} 입력을 고치면 저장할 수 있습니다.</p>}
    {error&&<p role="alert" className="text-danger text-xs">{error}</p>}
    {install&&<RuntimeInstaller name={name} onClose={()=>setInstall(false)} onInstalled={item=>{setChoices(items=>[...items,item]);selectProgram(item.path);setInstall(false);}}/>}
  </div>;
}
function RuntimeInstaller({name,onClose,onInstalled}:{name:string;onClose:()=>void;onInstalled:(entry:Executable)=>void}){
 const client=useOperationsClient();const [runtime,setRuntime]=useState(["node","java","go","bun"].includes(name)?name:"node");const [releases,setReleases]=useState<RuntimeRelease[]>([]);const [version,setVersion]=useState("");const [directory,setDirectory]=useState("");const [root,setRoot]=useState("");const [all,setAll]=useState(false);const [busy,setBusy]=useState(false);const [error,setError]=useState("");
 useEffect(()=>{client.developerTool<{runtime_root:string}>({operation:"paths"}).then(r=>setRoot(r.runtime_root)).catch(e=>setError(toErrorMessage(e)));},[]);
 useEffect(()=>{setBusy(true);setError("");client.developerTool<RuntimeRelease[]>({operation:"versions",name:runtime,all}).then(rows=>{setReleases(rows);setVersion(rows[0]?.version||"");}).catch(e=>setError(toErrorMessage(e))).finally(()=>setBusy(false));},[runtime,all]);
 useEffect(()=>{if(root&&version)setDirectory(`${root}/${runtime}/${version}`);},[root,runtime,version]);
 return <Modal title="실행자 설치" onClose={()=>{if(!busy)onClose();}}><div className="editor-body"><label className="editor-field"><span>실행자</span><select value={runtime} disabled={busy} onChange={e=>setRuntime(e.target.value)}>{["node","java","go","bun"].map(value=><option key={value}>{value}</option>)}</select></label><label className="editor-field"><span>버전</span><select value={version} disabled={busy} onChange={e=>setVersion(e.target.value)}>{releases.map(item=><option value={item.version} key={item.version}>{item.label}</option>)}</select></label><label className="editor-check"><input type="checkbox" checked={all} onChange={e=>setAll(e.target.checked)}/>전체 버전 보기</label><PathField label="설치 경로" value={directory} onChange={setDirectory}/><p className="editor-note">공식 배포 파일의 SHA-256을 검증한 뒤 지정한 빈 폴더에 설치합니다.</p>{error&&<p role="alert" className="text-danger">{error}</p>}</div><footer className="editor-footer"><Button onClick={onClose} disabled={busy}>취소</Button><Button variant="primary" disabled={busy||!version||!directory} onClick={()=>{setBusy(true);client.developerTool<Executable>({operation:"install",name:runtime,version,directory}).then(onInstalled).catch(e=>setError(toErrorMessage(e))).finally(()=>setBusy(false));}}>{busy?"처리 중…":"설치"}</Button></footer></Modal>;
}
