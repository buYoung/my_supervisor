import type {ProcessOperation,ProcessInstanceStatus} from "../../shared/types";
const outcomeLabels:Record<string,string>={completed:"완료",failed:"실패",not_attempted:"미시도",superseded:"이후 작업으로 대체됨"};
export function OperationResult({operation,instances}:{operation:ProcessOperation;instances:ProcessInstanceStatus[]}){
 const completed=operation.outcomes.filter(item=>item.state==="completed").length;
 const failed=operation.outcomes.filter(item=>item.state==="failed").length;
 const pending=operation.kind==="rolling_restart"?Math.max(operation.outcomes.filter(item=>item.state==="not_attempted").length,(operation.targetInstances??0)-completed-failed):operation.outcomes.filter(item=>item.state==="not_attempted").length;
 const protection=operation.compensation==="old healthy instance retained after readiness failure"?"대체 인스턴스가 준비되지 않아 기존 정상 인스턴스를 유지했습니다.":operation.compensation==="old healthy instance retained after replacement spawn failure"?"대체 인스턴스 시작에 실패해 기존 정상 인스턴스를 유지했습니다.":operation.compensation;
 const hasFailure=operation.phase==="failed_closed"||failed>0||pending>0;
 return <section className={hasFailure?"workspace-alert operation-result":"workspace-notice operation-result"} aria-label="작업 결과"><h3>{operation.name} · {hasFailure?"완료하지 못한 작업이 있습니다":operation.completed?"작업 완료":"작업 진행 중"}</h3><p>완료 {completed} · 실패 {failed} · 미시도 {pending} · 상태 {operation.phase}</p>{operation.outcomes.map(item=><p key={item.instanceId}>#{item.ordinal} · {outcomeLabels[item.state]??`알 수 없는 상태 (${item.state})`}{item.failedStage?` · 실패 단계 ${item.failedStage}`:""}{item.retryable?" · 다시 시도 가능":""}</p>)}{operation.compensation&&<p>보호 조치: {protection}</p>}{hasFailure&&<p>현재 실행 중인 인스턴스 {instances.filter(item=>item.state==="running").length}개를 유지합니다. 현재 상태를 확인한 뒤 새 작업을 요청하세요.</p>}</section>;
}
