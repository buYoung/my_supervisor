import type {EventEnvelope} from "../services/operations-client";
const processStates:Record<string,string>={running:"실행 중",stopped:"중지됨",crashed:"종료 감지",starting:"시작 중",stopping:"중지 중"};
export function describeOperatorEvent(event:EventEnvelope){
 const payload=event.payload&&typeof event.payload==="object"?event.payload as Record<string,unknown>:{};
 const name=typeof payload.name==="string"?payload.name:"대상 알 수 없음";
 let message=event.eventType,tone:"success"|"warning"|"danger"|"neutral"="neutral";
 let href=typeof payload.name==="string"?`?page=logs&kind=${event.eventType.startsWith("job.")?"job":"process"}&source=${encodeURIComponent(name)}`:undefined;
 if(event.eventType==="process.state_changed"){
  const from=String(payload.from??"unknown"),to=String(payload.to??"unknown");message=`${name} · ${processStates[from]??`알 수 없는 상태 (${from})`} → ${processStates[to]??`알 수 없는 상태 (${to})`}`;
  tone=to==="crashed"?"danger":to==="running"?"success":to==="starting"||to==="stopping"?"warning":"neutral";
 }else if(event.eventType==="process.guard_changed"){
  const guard=payload.guard as Record<string,unknown>|undefined;const values=[guard?.liveness,guard?.readiness];
  tone=values.includes("unhealthy")?"danger":values.includes("unsupported")?"warning":"neutral";
  message=`${name} · 상태 검사 ${tone==="danger"?"실패":tone==="warning"?"지원하지 않음":"변경"}${guard?.last_error?` · ${guard.last_error}`:""}`;
  href=`?page=processes&selected=${encodeURIComponent(name)}`;
 }else if(event.eventType.startsWith("job.")){
  const labels:Record<string,string>={"job.run_scheduled":"실행 예약","job.run_started":"실행 시작","job.run_succeeded":"실행 성공","job.run_failed":"실행 실패","job.run_timed_out":"제한 시간 초과","job.run_cancelled":"실행 취소","job.run_skipped":"실행 건너뜀"};
  message=`${name} · ${labels[event.eventType]??event.eventType}`;tone=event.eventType==="job.run_failed"||event.eventType==="job.run_timed_out"?"danger":event.eventType==="job.run_succeeded"?"success":"neutral";
  if(payload.run_id&&href)href+=`&run=${encodeURIComponent(String(payload.run_id))}`;
 }
 return {message,tone,href,timestamp:event.timestamp,type:event.eventType};
}
