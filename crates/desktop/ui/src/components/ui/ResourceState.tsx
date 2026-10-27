import {Button} from "./primitives";
import type {PolledResource} from "../../services/use-operations";
export function ResourceState({resource,label}:{resource:Pick<PolledResource<unknown>,"isLoading"|"isRefreshing"|"errorMessage"|"isStale"|"refresh">&{partial?:boolean;failedPartitions?:string[]};label:string}){
 if(resource.isLoading)return <p role="status" className="editor-note">{label}을 불러오는 중…</p>;
 if(resource.errorMessage)return <div role="alert" className="workspace-alert"><p>{resource.isStale?"마지막으로 확인한 데이터를 표시합니다. ":"데이터를 불러오지 못했습니다. "}{resource.errorMessage}</p><Button onClick={()=>void resource.refresh()}>다시 불러오기</Button></div>;
 if(resource.partial)return <div role="status" className="workspace-alert"><p>일부 데이터를 불러오지 못했습니다. 확인된 항목만 표시합니다. {resource.failedPartitions?.join(", ")}</p><Button onClick={()=>void resource.refresh()}>누락된 항목 다시 확인</Button></div>;
 if(resource.isRefreshing)return <p role="status" className="editor-note">{label} 갱신 중 · 현재 확인된 값을 표시합니다.</p>;
 return null;
}
