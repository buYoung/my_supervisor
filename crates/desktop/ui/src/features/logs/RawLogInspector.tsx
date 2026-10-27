import {useState} from 'react';
import {Copy} from 'lucide-react';
import {Modal} from '../../components/ui/Modal';
import {Button} from '../../components/ui/primitives';
import {useDisplayTime} from '../../shared/display-time';
import {toErrorMessage} from '../../services/use-operations';
import type {LogEntry} from '../../services/log-mapping';
import {Level} from './LogRows';
import {logEntryUrl} from './log-navigation';
export function RawLogInspector({entry,onClose,onContext}:{entry:LogEntry;onClose:()=>void;onContext?:(entry:LogEntry)=>void}){
 const {dateTime}=useDisplayTime();const [notice,setNotice]=useState(''),[error,setError]=useState('');
 let formatted:string|undefined;try{formatted=JSON.stringify(JSON.parse(entry.raw.line),null,2);}catch{/* Plain text remains exact. */}
 return <Modal title="로그 원본" onClose={onClose}><div className="log-inspector"><div className="flex items-center gap-3"><Level value={entry.level}/><strong>{entry.source}</strong><span>{entry.raw.stream}</span></div><dl><dt>발생 시간</dt><dd>{dateTime(entry.raw.timestamp,true)}</dd><dt>종류</dt><dd>{entry.kind==='process'?'프로세스':'작업'}</dd>{entry.runId&&<><dt>실행 ID</dt><dd>{entry.runId}</dd></>}{entry.step&&<><dt>단계</dt><dd>{entry.step}</dd></>}{entry.instance&&<><dt>인스턴스</dt><dd>{entry.instance}</dd></>}<dt>시퀀스</dt><dd>{entry.raw.sequence}</dd></dl><h3>원본 내용</h3><pre data-raw-log>{entry.raw.line}</pre>{formatted&&<details><summary>읽기 좋게 정리한 JSON</summary><pre>{formatted}</pre></details>}{notice&&<p role="status">{notice}</p>}{error&&<p role="alert" className="text-danger">{error}</p>}<div className="flex flex-wrap justify-end gap-2"><Button onClick={()=>{setError('');void navigator.clipboard.writeText(entry.raw.line).then(()=>setNotice('원본 로그를 복사했습니다.')).catch(cause=>setError(toErrorMessage(cause)));}}><Copy size={14}/>원본 복사</Button>{onContext?<Button onClick={()=>onContext(entry)}>전후 2분 보기</Button>:<a href={logEntryUrl(entry,entry.raw.timestamp)}>전후 2분 보기</a>}</div></div></Modal>;
}
