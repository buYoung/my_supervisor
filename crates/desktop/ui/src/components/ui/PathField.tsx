import { FolderOpen, FolderSearch } from "lucide-react";
import { useOperationsClient } from "../../services/use-operations";
import {useState} from "react";
export async function choosePath(directory:boolean,defaultPath?:string):Promise<string|null>{
  if(!(window as unknown as {__TAURI_INTERNALS__?:unknown}).__TAURI_INTERNALS__)throw new Error("폴더·파일 선택은 데스크톱 앱에서 사용할 수 있습니다. 브라우저 프리뷰에서는 경로를 직접 입력하세요.");
  const {open}=await import("@tauri-apps/plugin-dialog");const value=await open({directory,multiple:false,defaultPath});return typeof value==="string"?value:null;
}
export function PathField({value,onChange,label="작업 디렉터리",directory=true,readOnly=false}:{value:string;onChange?:(value:string)=>void;label?:string;directory?:boolean;readOnly?:boolean}){
  const client=useOperationsClient();
  const [error,setError]=useState("");
  return <label className="editor-field"><span>{label}</span><div className="path-field-row"><input value={value} readOnly={readOnly||!onChange} onChange={e=>onChange?.(e.target.value)} placeholder={readOnly?"설정되지 않음":"절대 경로를 입력하세요"}/>{!readOnly&&onChange&&<button type="button" title={directory?"폴더 선택":"실행 파일 선택"} onClick={()=>void choosePath(directory,value||undefined).then(path=>{if(path)onChange(path);setError("");}).catch(e=>setError(e.message))}><FolderSearch size={16}/></button>}{directory&&value&&<button type="button" title="폴더 열기" onClick={()=>void client.developerTool({operation:"open_folder",path:value}).catch(e=>setError(e.message))}><FolderOpen size={16}/></button>}</div>{error&&<small className="editor-note" role="status">{error}</small>}</label>;
}
