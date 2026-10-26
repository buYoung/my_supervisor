(() => {
  const cache = new Map();
  let resolveWorkflow=job=>job.workflow||{nodes:[],edges:[]};
  const copy = value => structuredClone(value);
  const identity = job => job.flowId || job.id || job.name;
  const storageKey = id => `msv-mock-job-versions-${id}`;
  const canonical = value => Array.isArray(value) ? value.map(canonical) : value && typeof value === 'object' ? Object.fromEntries(Object.keys(value).sort().filter(key=>value[key]!==undefined).map(key=>[key,canonical(value[key])])) : value;
  const stringify = value => JSON.stringify(canonical(value));

  function snapshot(job) {
    return copy({name:job.name,command:job.command||'',cwd:job.cwd||'',environment:job.environment||{},executable:job.executable||null,triggerType:job.triggerType,triggerValue:job.triggerValue||'',policy:job.policy||'skip',timezone:job.timezone||'Asia/Seoul',timeoutSeconds:job.timeoutSeconds||'',onDependencyFailure:job.onDependencyFailure||'skip',connectionGroupName:job.connectionGroupName||'',scheduleBeforeDependency:job.scheduleBeforeDependency||null,workflowGroupId:job.workflowGroupId||null,groupPolicy:job.groupPolicy||null,workflow:job.workflow||null});
  }
  function list(id) {
    if(!cache.has(id)) {
      try { cache.set(id,JSON.parse(sessionStorage.getItem(storageKey(id)))||[]); }
      catch { cache.set(id,[]); }
    }
    const records=copy(cache.get(id));
    return records.map((version,index)=>({...version,changes:changes(records[index+1]?.snapshot,version.snapshot)}));
  }
  function changes(before,after) {
    if(!before)return [{label:'현재 설정을 첫 버전으로 기록',kind:'initial'}];
    const result=[];
    const fields={workflowGroupId:'소속 그룹',groupPolicy:'그룹 실행 정책',name:'작업 이름',command:'실행 명령',cwd:'작업 디렉터리',environment:'환경변수',executable:'실행자',triggerType:'시작 방식',triggerValue:'실행 일정·선행 작업',policy:'중복 실행 정책',timezone:'시간대',timeoutSeconds:'제한 시간',onDependencyFailure:'선행 작업 실패 처리',connectionGroupName:'연결 그룹 이름'};
    for(const [key,label] of Object.entries(fields))if(stringify(before[key])!==stringify(after[key])) {
      const describe=value=>key==='environment'?`${Object.keys(value||{}).length}개 항목`:key==='executable'?'실행자 설정':String(value||'미설정');
      result.push({label,kind:'changed',before:describe(before[key]),after:describe(after[key])});
    }
    const oldWorkflow=resolveWorkflow(before),newWorkflow=resolveWorkflow(after);
    const oldNodes=new Map((oldWorkflow.nodes||[]).map(node=>[node.id,node]));
    const newNodes=new Map((newWorkflow.nodes||[]).map(node=>[node.id,node]));
    for(const [id,node] of newNodes) {
      if(!oldNodes.has(id))result.push({kind:'added',label:`단계 추가 · ${node.data.label}`});
      else if(stringify(oldNodes.get(id).data)!==stringify(node.data)) {
        const labels={resultRoutes:'결과별 경로',label:'이름',command:'명령',cwd:'작업 디렉터리',environment:'환경변수',operation:'제어 동작',processName:'대상 프로세스',url:'확인 URL',expectedStatus:'HTTP 상태 코드',host:'호스트',port:'포트',checkType:'확인 방식',seconds:'대기 시간',timeoutSeconds:'제한 시간',maxAttempts:'총 시도 횟수',retryDelaySeconds:'재시도 간격',source:'조건 대상',operator:'비교 방식',value:'조건 값',path:'경로',result:'종료 결과'};
        const oldData=oldNodes.get(id).data,keys=new Set([...Object.keys(oldData),...Object.keys(node.data)]);
        const modified=[...keys].filter(key=>stringify(oldData[key])!==stringify(node.data[key])).map(key=>labels[key]||key);
        result.push({kind:'changed',label:`단계 설정 · ${node.data.label}`,detail:modified.join(', ')});
      }
    }
    for(const [id,node] of oldNodes)if(!newNodes.has(id))result.push({kind:'removed',label:`단계 삭제 · ${node.data.label}`});
    const edgeKey=edge=>`${edge.source}/${edge.sourceHandle||'out'}/${edge.target}`;
    const oldEdges=new Map((oldWorkflow.edges||[]).map(edge=>[edgeKey(edge),edge]));
    const newEdges=new Map((newWorkflow.edges||[]).map(edge=>[edgeKey(edge),edge]));
    for(const [key,edge] of newEdges)if(!oldEdges.has(key))result.push({kind:'added',label:`연결 추가 · ${newNodes.get(edge.source)?.data.label||'시작'} → ${newNodes.get(edge.target)?.data.label||'종료'}`});
    for(const [key,edge] of oldEdges)if(!newEdges.has(key))result.push({kind:'removed',label:`연결 해제 · ${oldNodes.get(edge.source)?.data.label||'시작'} → ${oldNodes.get(edge.target)?.data.label||'종료'}`});
    if([...newNodes].some(([id,node])=>oldNodes.has(id)&&stringify(node.position)!==stringify(oldNodes.get(id).position)))result.push({kind:'changed',label:'단계 배치 변경'});
    return result.length?result:[{kind:'changed',label:'흐름 설정 변경'}];
  }
  function record(job) {
    const id=identity(job),versions=list(id),value=snapshot(job),previous=versions[0];
    if(previous&&stringify(snapshot(previous.snapshot))===stringify(value))return previous;
    const version={id:`${id}:v${(previous?.number||0)+1}`,number:(previous?.number||0)+1,createdAt:new Date().toISOString(),snapshot:value,changes:changes(previous?.snapshot,value)};
    const next=[version,...versions].slice(0,50);
    cache.set(id,next);
    try{sessionStorage.setItem(storageKey(id),JSON.stringify(next));}catch{}
    window.dispatchEvent(new CustomEvent('supervisor-job-version-changed',{detail:{jobId:id}}));
    return copy(version);
  }
  function recordJobs(jobs) { for(const job of jobs){job.flowId=job.flowId||job.id||crypto.randomUUID();record(job);} }
  function clear() { cache.clear(); }
  window.SupervisorJobVersions={configure:resolver=>{resolveWorkflow=resolver;},list,current:id=>list(id)[0]||null,record,recordJobs,snapshot,changes,identity,clear};
})();
