import {initialWorkflow,workflowIssues} from './workflow-model.js';
import {cancelWorkflowRun,getWorkflowRun,startWorkflowRun,subscribeWorkflowRun} from './workflow-runner.js';
import {groupPolicyError} from './group-model.js';

const histories=new Map(),controllers=new Map(),listeners=new Map();
const clone=value=>structuredClone(value);
export function groupRuns(id){if(!histories.has(id)){let records=[];try{records=JSON.parse(sessionStorage.getItem(`msv-group-runs-${id}`))||[];}catch{}for(const record of records)if(record.state==='running'){record.state='cancelled';for(const job of Object.values(record.jobs))if(['pending','running'].includes(job.state))job.state='cancelled';}histories.set(id,records);}return clone(histories.get(id));}
function publish(run){const records=groupRuns(run.groupId),index=records.findIndex(item=>item.id===run.id);if(index<0)records.unshift(clone(run));else records[index]=clone(run);histories.set(run.groupId,records.slice(0,30));try{sessionStorage.setItem(`msv-group-runs-${run.groupId}`,JSON.stringify(records.slice(0,30)));}catch{}for(const callback of listeners.get(run.groupId)||[])callback(clone(run));}
export function subscribeGroupRun(id,callback){if(!listeners.has(id))listeners.set(id,new Set());listeners.get(id).add(callback);return ()=>listeners.get(id)?.delete(callback);}
export function cancelGroupRun(id){controllers.get(id)?.abort();}
export function groupRunError(group,jobs,processes){
  const members=jobs.filter(job=>group.nodeIds.includes(job.id));
  const error=groupPolicyError(group.policy);if(error)return error;
  for(const job of members){if(getWorkflowRun(job.id)?.state==='running')return `${job.name} 작업이 이미 실행 중입니다.`;const issue=workflowIssues(initialWorkflow(job),job.id,processes)[0];if(issue)return `${job.name}: ${issue.message}`;}
  return '';
}
export async function startGroupRun(group,jobs,processes){
  if(controllers.has(group.id))return;
  const error=groupRunError(group,jobs,processes);if(error)throw new Error(error);
  const members=clone(jobs.filter(job=>group.nodeIds.includes(job.id))),previous=groupRuns(group.id),controller=new AbortController();controllers.set(group.id,controller);
  const startedAt=new Date().toISOString(),run={id:`group-run-${crypto.randomUUID()}`,groupId:group.id,name:group.name,sequence:(previous[0]?.sequence||0)+1,state:'running',startedAt,finishedAt:null,policy:clone(group.policy),snapshot:members,jobs:Object.fromEntries(members.map(job=>[job.id,{state:'pending',runId:null,summary:'선행 작업 대기'}]))};
  const pending=new Set(members.map(job=>job.id)),active=new Map();let hasFailed=false,hasTimedOut=false;
  const cancel=()=>{for(const id of active.keys())cancelWorkflowRun(id);};controller.signal.addEventListener('abort',cancel);
  const timer=Number(group.policy.timeoutSeconds)>0?setTimeout(()=>{hasTimedOut=true;controller.abort();},Number(group.policy.timeoutSeconds)*1000):null;
  publish(run);
  const launch=job=>{
    pending.delete(job.id);run.jobs[job.id]={...run.jobs[job.id],state:'running',summary:'내부 흐름 실행 중'};publish(run);
    const unsubscribe=subscribeWorkflowRun(job.id,child=>{run.jobs[job.id]={state:child.state,runId:child.id,summary:child.activeNodeId?child.steps[child.activeNodeId]?.label:child.state,startedAt:child.startedAt,durationMs:child.durationMs};publish(run);});
    const promise=startWorkflowRun({job,workflow:initialWorkflow(job),groupName:group.name}).then(child=>{run.jobs[job.id]={state:child.state,runId:child.id,summary:child.state==='failed'?'내부 흐름 실패':child.state==='cancelled'?'실행 취소':'완료',startedAt:child.startedAt,durationMs:child.durationMs};if(child.state==='failed')hasFailed=true;}).finally(()=>{unsubscribe();active.delete(job.id);publish(run);});
    active.set(job.id,promise);
  };
  try {
    while(pending.size||active.size){
      if(controller.signal.aborted||hasFailed&&group.policy.failurePolicy==='stop'){
        for(const id of pending){run.jobs[id].state='skipped';run.jobs[id].summary=controller.signal.aborted?'그룹 실행 중단':'다른 작업 실패로 시작하지 않음';}pending.clear();
        if(controller.signal.aborted)cancel();
      }else{
        for(const id of [...pending]){
          const job=members.find(item=>item.id===id),parents=(job.upstream||[]).filter(id=>group.nodeIds.includes(id));
          const complete=parents.every(id=>['succeeded','failed','cancelled','skipped'].includes(run.jobs[id].state));
          if(!complete)continue;
          if(parents.some(id=>run.jobs[id].state!=='succeeded')&&job.onDependencyFailure!=='run_anyway'){pending.delete(id);run.jobs[id]={state:'skipped',summary:'선행 작업 실패'};publish(run);continue;}
          if(active.size<Number(group.policy.maxParallel))launch(job);
        }
      }
      if(active.size)await Promise.race([...active.values()]);
      else if(pending.size){for(const id of pending)run.jobs[id]={state:'skipped',summary:'해결되지 않은 선행 관계'};pending.clear();hasFailed=true;}
    }
    run.state=controller.signal.aborted?'cancelled':hasFailed?'failed':'succeeded';run.summary=hasTimedOut?'그룹 제한 시간을 초과했습니다.':run.state==='succeeded'?'그룹 실행 완료':run.state==='failed'?'실패한 작업이 있습니다.':'그룹 실행 취소';
  }finally{clearTimeout(timer);controller.signal.removeEventListener('abort',cancel);controllers.delete(group.id);run.finishedAt=new Date().toISOString();run.durationMs=Date.parse(run.finishedAt)-Date.parse(startedAt);publish(run);}
  return clone(run);
}
