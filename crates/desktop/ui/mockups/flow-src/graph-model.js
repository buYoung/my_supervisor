export const upstreamNames = job => job.triggerType === 'depends_on' ? String(job.triggerValue || job.expr || '').split(',').map(name => name.trim()).filter(Boolean) : [];

export function toDraft(records) {
  const ids = new Map(records.map(job => [job.name, job.flowId || job.name]));
  return records.map(job => ({...job, id:ids.get(job.name), upstream:upstreamNames(job).map(name=>ids.get(name)).filter(Boolean), cwd:job.cwd||'', environment:job.environment||{}, timezone:job.timezone||'Asia/Seoul', timeoutSeconds:job.timeoutSeconds||'', onDependencyFailure:job.onDependencyFailure||'skip'}));
}

export function definition(jobs) {
  return JSON.stringify(jobs.map(({id,name,command,cwd,environment,executable,triggerType,triggerValue,upstream,policy,timezone,timeoutSeconds,onDependencyFailure,workflow,connectionGroupName,workflowGroupId,groupPolicy})=>({id,name,command,cwd,environment,executable,triggerType,triggerValue,upstream,policy,timezone,timeoutSeconds,onDependencyFailure,workflow,connectionGroupName,workflowGroupId,groupPolicy})));
}

export function toRecords(jobs) {
  const names=new Map(jobs.map(job=>[job.id,job.name.trim()]));
  return jobs.map(({id,upstream,...job})=>({...job,name:job.name.trim(),flowId:id,triggerValue:job.triggerType==='depends_on'?upstream.map(id=>names.get(id)).join(', '):job.triggerValue}));
}

export function connectionError(jobs, source, target) {
  if(!source||!target)return '연결할 두 작업을 선택하세요.';
  if(source===target)return '자기 자신을 선행 작업으로 연결할 수 없습니다.';
  const targetJob=jobs.find(job=>job.id===target);
  if(!targetJob||!jobs.some(job=>job.id===source))return '연결할 작업을 찾을 수 없습니다.';
  if(targetJob.upstream.includes(source))return '이미 연결된 선행 작업입니다.';
  const visited=new Set(),pending=[source];
  while(pending.length){const id=pending.pop();if(id===target)return '순환 의존 관계가 생깁니다. 다른 작업을 선택하세요.';if(visited.has(id))continue;visited.add(id);pending.push(...(jobs.find(job=>job.id===id)?.upstream||[]));}
  return '';
}

export function connectJobs(jobs, source, target) {
  return jobs.map(job=>job.id!==target?job:{...job,upstream:[...job.upstream,source],triggerType:'depends_on',scheduleBeforeDependency:job.triggerType!=='depends_on'?{triggerType:job.triggerType,triggerValue:job.triggerValue}:job.scheduleBeforeDependency});
}

export function disconnectJobs(jobs, source, target) {
  return jobs.map(job=>{
    if(job.id!==target)return job;
    const upstream=job.upstream.filter(id=>id!==source);
    return {...job,...(!upstream.length&&job.scheduleBeforeDependency?job.scheduleBeforeDependency:{}),upstream};
  });
}

export function validationErrors(jobs) {
  const errors=[];const names=new Set();
  for(const job of jobs){
    const name=job.name.trim();
    if(!name)errors.push({id:job.id,field:'name',message:'작업 이름을 입력하세요.'});
    else if(names.has(name))errors.push({id:job.id,field:'name',message:`${name}: 이미 사용 중인 이름입니다.`});
    names.add(name);
    if(!job.workflow&&!job.command?.trim())errors.push({id:job.id,field:'command',message:`${name}: 실행 명령을 입력하세요.`});
    if(job.triggerType==='depends_on'&&!job.upstream.length)errors.push({id:job.id,field:'triggerType',message:`${name}: 선행 작업을 연결하거나 실행 조건을 선택하세요.`});
    if(job.triggerType==='cron'&&String(job.triggerValue||'').trim().split(/\s+/).length!==5)errors.push({id:job.id,field:'triggerValue',message:`${name}: cron 표현식은 5개 필드로 입력하세요.`});
    if(job.triggerType==='interval'&&(!Number.isInteger(Number(job.triggerValue))||Number(job.triggerValue)<1))errors.push({id:job.id,field:'triggerValue',message:`${name}: 실행 간격은 1초 이상의 정수여야 합니다.`});
    if(job.triggerType==='one_shot'&&(!job.triggerValue||Number.isNaN(Date.parse(job.triggerValue))))errors.push({id:job.id,field:'triggerValue',message:`${name}: 실행 시각을 지정하세요.`});
    if(job.timeoutSeconds!==''&&(!Number.isInteger(Number(job.timeoutSeconds))||Number(job.timeoutSeconds)<1))errors.push({id:job.id,field:'timeoutSeconds',message:`${name}: 제한 시간은 1초 이상의 정수로 입력하세요.`});
    for(const source of job.upstream){
      const without=jobs.map(item=>item.id===job.id?{...item,upstream:item.upstream.filter(id=>id!==source)}:item);
      const error=connectionError(without,source,job.id);
      if(error){errors.push({id:job.id,field:'triggerType',message:`${name}: ${error}`});break;}
    }
  }
  return errors;
}

export function autoLayout(jobs) {
  const rank=new Map(),visiting=new Set();
  const depth=id=>{if(rank.has(id))return rank.get(id);if(visiting.has(id))return 0;visiting.add(id);const parents=jobs.find(job=>job.id===id)?.upstream||[];const value=parents.length?Math.max(...parents.map(depth))+1:0;visiting.delete(id);rank.set(id,value);return value;};
  jobs.forEach(job=>depth(job.id));
  const groups=window.SupervisorFlowGroups.jobGroups(jobs),connected=new Set(groups.flatMap(group=>group.nodeIds));
  const positions={};let top=58;
  for(const group of groups){
    const columns=new Map();let maximumRows=1;
    for(const job of jobs.filter(job=>group.nodeIds.includes(job.id))){const column=rank.get(job.id),row=columns.get(column)||0;positions[job.id]={x:24+column*300,y:top+row*166};columns.set(column,row+1);maximumRows=Math.max(maximumRows,row+1);}
    top+=maximumRows*166+82;
  }
  jobs.filter(job=>!connected.has(job.id)).forEach((job,index)=>{positions[job.id]={x:24+(index%2)*300,y:(groups.length?top:24)+Math.floor(index/2)*166};});
  return positions;
}

export function triggerLabel(job) {
  if(job.triggerType==='depends_on')return job.upstream.length?`선행 ${job.upstream.length}개 · 모두 완료 후`:'실행 조건 필요';
  if(job.triggerType==='interval')return `${job.triggerValue}초마다`;
  if(job.triggerType==='one_shot')return '지정 시각에 한 번';
  return job.triggerValue==='0 2 * * *'?'매일 02:00':job.triggerValue==='0 0 * * *'?'매일 00:00':job.triggerValue==='0 4 * * 0'?'매주 일요일 04:00':job.expr===job.triggerValue?job.trigger||job.triggerValue:job.triggerValue||'예약 일정';
}
