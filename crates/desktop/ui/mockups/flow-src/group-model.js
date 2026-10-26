import {connectJobs,connectionError,disconnectJobs} from './graph-model.js';
export const defaultGroupPolicy={maxParallel:4,failurePolicy:'continue',timeoutSeconds:''};
export function groupForJob(jobs,ownerId){
  const group=window.SupervisorFlowGroups.jobGroups(jobs).find(group=>group.nodeIds.includes(ownerId)),owner=jobs.find(job=>job.id===ownerId);
  const ids=group?.nodeIds||[ownerId],existing=jobs.find(job=>ids.includes(job.id)&&job.workflowGroupId);
  return {id:existing?.workflowGroupId||`group:${ids.slice().sort()[0]}`,name:group?.name||`${owner?.name||'작업'} 그룹`,nodeIds:ids,policy:{...defaultGroupPolicy,...owner?.groupPolicy}};
}
export function includeGroupJobs(jobs,ownerId,incomingIds){
  const group=groupForJob(jobs,ownerId),ids=new Set(group.nodeIds);
  for(const id of incomingIds)groupForJob(jobs,id).nodeIds.forEach(member=>ids.add(member));
  return jobs.map(job=>ids.has(job.id)?{...job,workflowGroupId:group.id,connectionGroupName:group.name,groupPolicy:group.policy}:job);
}
export function groupDefinition(jobs){return JSON.stringify(jobs.map(({id,upstream,triggerType,triggerValue,workflowGroupId,connectionGroupName,groupPolicy})=>({id,upstream,triggerType,triggerValue,workflowGroupId,connectionGroupName,groupPolicy})));}
export function connectGroupJobs(jobs,ownerId,source,target){
  const error=connectionError(jobs,source,target);if(error)return {error};
  const linked=connectJobs(jobs,source,target);
  return {jobs:includeGroupJobs(linked,ownerId,[source,target])};
}
export function disconnectGroupJobs(jobs,source,target){return disconnectJobs(jobs,source,target);}
export function groupPolicyError(policy){
  if(!Number.isInteger(Number(policy.maxParallel))||Number(policy.maxParallel)<1||Number(policy.maxParallel)>16)return '동시 실행 작업 수는 1~16개로 입력하세요.';
  if(policy.timeoutSeconds!==''&&(!Number.isInteger(Number(policy.timeoutSeconds))||Number(policy.timeoutSeconds)<1))return '그룹 제한 시간은 1초 이상의 정수로 입력하세요.';
  return '';
}
export function groupLayout(jobs){
  const depths=new Map(),visiting=new Set();
  const depth=id=>{if(depths.has(id))return depths.get(id);if(visiting.has(id))return 0;visiting.add(id);const parents=(jobs.find(job=>job.id===id)?.upstream||[]).filter(id=>jobs.some(job=>job.id===id));const value=parents.length?1+Math.max(...parents.map(depth)):0;visiting.delete(id);depths.set(id,value);return value;};
  const rows=new Map(),positions={};
  for(const job of jobs){const column=depth(job.id),row=rows.get(column)||0;positions[job.id]={x:30+column*350,y:40+row*245};rows.set(column,row+1);}
  return positions;
}
