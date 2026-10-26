/* Data adapters for the local log mockup. Archived job runs below are explicit fixtures. */
(() => {
  const levels=['fatal','error','warn','info','debug','trace','unknown'];
  const labels={fatal:'FATAL',error:'ERROR',warn:'WARN',info:'INFO',debug:'DEBUG',trace:'TRACE',unknown:'미분류'};
  const severity=value=>{
    if(typeof value==='number')return ({10:'trace',20:'debug',30:'info',40:'warn',50:'error',60:'fatal'})[value]||null;
    const text=String(value??'').toLowerCase();return ({warning:'warn',err:'error',critical:'fatal',crit:'fatal'})[text]||(levels.includes(text)?text:null);
  };
  function levelOf(record){
    const explicit=severity(record.level);if(explicit)return {level:explicit,levelSource:'명시된 level 필드'};
    try {
      const json=JSON.parse(record.message);
      const level=severity(json.level??json.severity??json.severityText??json['log.level']);
      if(level)return {level,levelSource:'구조화된 로그 필드'};
    }catch{}
    const match=String(record.message).match(/^(?:\d{4}-\d\d-\d\d[T ][\d:.Z+\-]+\s+)?\[?(FATAL|ERROR|WARN(?:ING)?|INFO|DEBUG|TRACE)\]?(?:\s|:)/i)||String(record.message).match(/(?:^|\s)level=(fatal|error|warn|warning|info|debug|trace)(?:\s|$)/i);
    if(match)return {level:severity(match[1]),levelSource:'메시지의 수준 표기'};
    return {level:'unknown',levelSource:'수준 정보 없음 · 출력 경로만으로 판단하지 않음'};
  }
  const isAttention=record=>['fatal','error','warn','unknown'].includes(record.level);
  const timeParts=timestamp=>{const text=new Date(timestamp+9*3600000).toISOString();return {date:text.slice(0,10),month:text.slice(0,7),localTime:text.slice(11,19)};};
  function normalize(record){
    const raw=String(record.raw??record.message??'');let message=String(record.message??'');
    try{const parsed=JSON.parse(message);message=String(parsed.message??parsed.msg??message);}catch{}
    return {...record,raw,message,...timeParts(record.timestamp),...levelOf(record),...(record.levelSource?{levelSource:record.levelSource}:{})};
  }
  function groupRepeated(records){
    const groups=new Map();
    for(const record of records){
      const key=JSON.stringify([record.kind,record.sourceId,record.instanceId??record.instance??record.pid??'',record.runId||'',record.stageId||'',record.level,record.stream,record.message]);
      let group=groups.get(key);
      if(!group){group={key,records:[],first:record,last:record};groups.set(key,group);}
      group.records.push(record);
      if(record.timestamp<group.first.timestamp)group.first=record;
      if(record.timestamp>group.last.timestamp)group.last=record;
    }
    return [...groups.values()];
  }
  function systemLevel(message,run){
    if(/^단계 제한 시간을 초과했습니다/.test(message))return 'error';
    if(/^서킷 차단|^연속 실패 임계값 도달|^\d+(\.\d+)?초 후 이 단계만 재시도|^실패를 기록|^실패 결과를/.test(message))return 'warn';
    if(message==='실행이 취소되었습니다.'&&run.state==='cancelled')return 'warn';
    return 'info';
  }

  function createJobArchive(jobs,firstDate,lastDate){
    const runs=[];
    let dayIndex=0;
    for(let timestamp=Date.parse(`${firstDate}T00:00:00+09:00`);timestamp<=Date.parse(`${lastDate}T00:00:00+09:00`);timestamp+=86400000,dayIndex++){
      jobs.forEach((job,jobIndex)=>{
        const start=timestamp+(2+jobIndex*2)*3600000,hasFailed=(dayIndex+jobIndex*3)%19===0,id=`archive-${job.name}-${timeParts(start).date}`;
        const names={start:'예약 실행',execute:'명령 실행',check:'종료 코드 확인',success:'성공으로 종료',failure:'실패로 종료'};
        const log=(offset,stream,level,message)=>({time:new Date(start+offset).toISOString(),stream,level,message});
        const status=hasFailed?'failed':'succeeded',terminal=hasFailed?'failure':'success';
        const steps={
          start:{id:'start',kind:'trigger',label:names.start,state:'succeeded',startedAt:new Date(start).toISOString(),exitCode:0,logs:[log(0,'system','info','예약된 작업 실행을 시작합니다.')]},
          execute:{id:'execute',kind:'command',label:names.execute,state:status,isFailureHandled:hasFailed,attempt:hasFailed?2:1,startedAt:new Date(start+100).toISOString(),exitCode:hasFailed?1:0,logs:[log(100,'system','info',`실행 명령: ${job.command}`),...(hasFailed?[log(2500,'stderr','error','대상 경로에 연결하지 못했습니다. ECONNRESET'),log(2600,'system','warn','1초 후 재시도 · 시도 2/2'),log(5400,'stderr','error','재시도 실패 · 원격 연결이 종료되었습니다.')]:[log(2600,'stdout','info','작업 처리를 완료했습니다.'),log(2800,'system','debug','result_bytes=240 exit_code=0')])]},
          check:{id:'check',kind:'condition',label:names.check,state:'succeeded',branch:hasFailed?'false':'true',startedAt:new Date(start+6000).toISOString(),exitCode:0,logs:[log(6000,'system','info',`종료 코드 ${hasFailed?1:0} → ${hasFailed?'거짓':'참'} 경로`)]},
          success:{id:'success',kind:'finish',label:names.success,state:hasFailed?'skipped':'succeeded',startedAt:hasFailed?null:new Date(start+7000).toISOString(),exitCode:hasFailed?null:0,logs:hasFailed?[]:[log(7000,'system','info','작업 실행 완료')]},
          failure:{id:'failure',kind:'finish',label:names.failure,state:hasFailed?'failed':'skipped',startedAt:hasFailed?new Date(start+7000).toISOString():null,exitCode:hasFailed?1:null,logs:hasFailed?[log(7000,'system','error','실패 경로로 작업을 종료했습니다.')]:[]},
        };
        const edges=[['start','execute','out'],['execute','check','out'],['check','success','true'],['check','failure','false']].map(([source,target,sourceHandle])=>({id:`${source}-${target}`,source,target,sourceHandle}));
        runs.push({id,jobId:job.flowId||job.name,jobName:job.name,sequence:dayIndex+1,scope:'flow',state:status,isArchiveSample:true,versionLabel:'보관 샘플 · v1',startedAt:new Date(start).toISOString(),finishedAt:new Date(start+7100).toISOString(),durationMs:7100,failedNodeId:hasFailed?'failure':null,steps,visitedEdges:['start-execute','execute-check',`check-${terminal}`],workflow:{nodes:Object.entries(names).map(([nodeId,label])=>({id:nodeId,data:{kind:steps[nodeId].kind,label,...(nodeId==='execute'?{command:job.command}:{}),...(nodeId==='check'?{source:'exit_code',value:'0'}:{})}})),edges}});
      });
    }
    return runs;
  }

  function create(options){
    const extras=[],archive=createJobArchive(options.archiveJobs,options.firstDate,options.lastDate);
    function read(){
      const sources={process:options.processes.map(process=>({id:process.name,name:process.name,state:process.state})),job:options.jobs.map(job=>({id:job.flowId||job.name,name:job.name,state:job.state}))};
      const allowedJobs=new Set(sources.job.map(source=>source.id));
      const runs=[...archive.filter(run=>allowedJobs.has(run.jobId)),...options.jobs.flatMap(job=>window.SupervisorRunPresentation?.list(job)||[])];
      const byRun=new Map(runs.map(run=>[run.id,run]));
      const processRecords=options.records.map(record=>normalize({...record,kind:'process',sourceId:record.source,sourceName:record.source}));
      const jobRecords=runs.flatMap(run=>Object.values(run.steps||{}).flatMap(step=>(step.logs||[]).map((entry,index)=>normalize({...entry,id:`${run.id}:${step.id}:${index}`,timestamp:Date.parse(entry.time),kind:'job',sourceId:run.jobId,sourceName:run.jobName,runId:run.id,runLabel:`#${String(run.sequence).padStart(3,'0')}`,runState:run.state,version:run.versionLabel,isArchiveSample:run.isArchiveSample,stageId:step.id,stageName:step.label,stageKind:step.kind,level:entry.level||(entry.stream==='system'?systemLevel(entry.message,run):null),levelSource:!entry.level&&entry.stream==='system'?'작업 실행기의 시스템 이벤트':undefined}))));
      const records=[...processRecords,...jobRecords,...extras];
      const dates=records.map(record=>record.date).sort();
      return {records,runs:byRun,sources,firstDate:dates[0]||options.firstDate,lastDate:dates.at(-1)||options.lastDate};
    }
    return {read,append:record=>extras.push(normalize(record))};
  }
  window.SupervisorLogData={create,levels,labels,isAttention,levelOf,normalize,groupRepeated,dateOf:timestamp=>timeParts(timestamp).date};
})();
