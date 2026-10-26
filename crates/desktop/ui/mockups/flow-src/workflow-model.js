import {resultRoutes,resultRoutingError,sourcePorts,supportsResultRoutes} from './result-routing.js';
import {policyError} from './execution-policy.js';
import {parallelIssues} from './parallel-flow.js';

export const stepKinds={
  trigger:{label:'시작',icon:'Clock3',group:'시작'},
  command:{label:'명령 실행',icon:'Terminal',group:'실행'},
  condition:{label:'조건 분기',icon:'ArrowRightLeft',group:'조건'},
  wait:{label:'대기',icon:'Clock3',group:'동작'},
  process:{label:'프로세스 제어',icon:'RefreshCw',group:'동작'},
  health:{label:'준비 상태 확인',icon:'ShieldCheck',group:'확인'},
  job:{label:'다른 작업 호출',icon:'ArrowUpRight',group:'동작'},
  finish:{label:'종료',icon:'CheckCircle2',group:'결과'},
  parallel:{label:'병렬 실행',icon:'ArrowRightLeft',group:'흐름 제어'},
  join:{label:'결과 합류',icon:'ArrowDownUp',group:'흐름 제어'},
};

export function initialWorkflow(job) {
  if(job.workflow?.nodes?.length)return structuredClone(job.workflow);
  return {version:1,nodes:[
    {id:'start',type:'step',position:{x:150,y:0},data:{kind:'trigger',label:'작업 시작',triggerType:job.triggerType,triggerValue:job.triggerValue,timezone:job.timezone||'Asia/Seoul'}},
    {id:'execute',type:'step',position:{x:150,y:150},data:{kind:'command',label:'명령 실행',command:job.command||'',cwd:job.cwd||'',environment:job.environment||{},executable:job.executable||null,timeoutSeconds:job.timeoutSeconds||''}},
    {id:'check',type:'step',position:{x:150,y:310},data:{kind:'condition',label:'정상 종료 확인',source:'exit_code',operator:'equals',value:'0',path:''}},
    {id:'success',type:'step',position:{x:0,y:490},data:{kind:'finish',label:'성공으로 종료',result:'succeeded',message:''}},
    {id:'failure',type:'step',position:{x:300,y:490},data:{kind:'finish',label:'실패로 종료',result:'failed',message:''}},
  ],edges:[
    {id:'start-execute',source:'start',sourceHandle:'out',target:'execute',targetHandle:'in'},
    {id:'execute-check',source:'execute',sourceHandle:'out',target:'check',targetHandle:'in'},
    {id:'check-success',source:'check',sourceHandle:'true',target:'success',targetHandle:'in'},
    {id:'check-failure',source:'check',sourceHandle:'false',target:'failure',targetHandle:'in'},
  ]};
}

export function stepSummary(data,jobs=[]) {
  if(data.kind==='trigger')return data.triggerType==='depends_on'?'선행 작업 완료 후':data.triggerType==='interval'?`${data.triggerValue}초마다`:data.triggerType==='one_shot'?data.triggerValue||'실행 시각 지정':`${data.triggerValue||'예약 일정'} · ${data.timezone||'Asia/Seoul'}`;
  if(data.kind==='command')return data.command||'실행할 명령을 입력하세요';
  if(data.kind==='condition')return data.source==='file_exists'?`파일 존재 · ${data.path||'경로 지정'}`:`종료 코드 ${data.operator==='equals'?'=':data.operator==='not_equals'?'≠':'≥'} ${data.value}`;
  if(data.kind==='wait')return `${data.seconds||0}초 후 다음 단계`;
  if(data.kind==='process')return `${data.processName||'프로세스 선택'} · ${{start:'시작',stop:'중지',restart:'재시작'}[data.operation]||'동작 선택'}`;
  if(data.kind==='health')return data.checkType==='tcp'?`TCP ${data.host}:${data.port}`:data.checkType==='process'?`${data.processName||'프로세스 선택'} · 실행 상태 확인`:`HTTP ${data.url||'주소 입력'}`;
  if(data.kind==='job')return jobs.find(job=>job.id===data.jobId)?.name||'호출할 작업을 선택하세요';
  if(data.kind==='parallel')return `${data.branches?.length||0}개 분기를 동시에 실행`;
  if(data.kind==='join')return data.joinMode==='race'?'Race · 첫 완료의 성공·실패 채택':data.joinMode==='first_success'?'첫 성공 후 나머지 분기 취소':'모든 분기의 실행 결과를 기다립니다';
  return data.result==='failed'?'작업을 실패로 마칩니다':'작업을 성공으로 마칩니다';
}

export function stepConnectionError(workflow, connection, exceptId) {
  const {source,target,sourceHandle='out'}=connection;
  const from=workflow.nodes.find(node=>node.id===source),to=workflow.nodes.find(node=>node.id===target);
  if(!from||!to)return '연결할 단계를 선택하세요.';
  if(source===target)return '자기 자신에게 연결할 수 없습니다.';
  if(from.data.kind==='finish'||to.data.kind==='trigger')return '시작은 출발점, 종료는 도착점으로 연결하세요.';
  if(!sourcePorts(from.data).some(port=>port.id===sourceHandle))return '존재하는 결과 경로에 연결하세요.';
  const edges=workflow.edges.filter(edge=>edge.id!==exceptId);
  if(edges.some(edge=>edge.source===source&&(edge.sourceHandle||'out')===sourceHandle))return supportsResultRoutes(from.data)?'이 결과 경로는 이미 연결되어 있습니다. 다른 결과는 결과별 경로에서 추가하세요.':'이 출구는 이미 연결되어 있습니다. 기존 연결을 선택해 중간에 단계를 넣거나 연결을 삭제하세요.';
  const seen=new Set(),pending=[target];
  while(pending.length){const id=pending.pop();if(id===source)return '반복되는 연결은 만들 수 없습니다.';if(seen.has(id))continue;seen.add(id);pending.push(...edges.filter(edge=>edge.source===id).map(edge=>edge.target));}
  return '';
}

export function stepExecutionError(data,ownerId,processes=[]) {
  const policyIssue=policyError(data);if(policyIssue)return policyIssue;
  if(data.kind==='command'&&!data.command?.trim())return '실행할 명령을 입력하세요.';
  if(supportsResultRoutes(data)){const error=resultRoutingError(data);if(error)return error;}
  if(data.kind==='wait'&&(!Number.isInteger(Number(data.seconds))||Number(data.seconds)<1))return '대기 시간을 1초 이상으로 입력하세요.';
  if(data.kind==='job'&&(!data.jobId||data.jobId===ownerId))return '자기 자신을 제외한 호출 대상을 선택하세요.';
  if(data.kind==='process'&&!processes.some(process=>process.name===data.processName))return '등록된 프로세스를 선택하세요.';
  if(data.kind==='process'&&!['start','stop','restart'].includes(data.operation))return '프로세스 제어 동작을 선택하세요.';
  if(data.kind==='health') {
    if(!['http','tcp','process'].includes(data.checkType))return '확인 방식을 선택하세요.';
    if(data.checkType==='http'){try{const url=new URL(data.url);if(!['http:','https:'].includes(url.protocol))throw new Error();}catch{return 'HTTP 또는 HTTPS 주소를 입력하세요.';}}
    if(data.checkType==='http'&&(!Number.isInteger(Number(data.expectedStatus))||Number(data.expectedStatus)<100||Number(data.expectedStatus)>599))return '예상 HTTP 상태 코드는 100~599로 입력하세요.';
    if(data.checkType==='tcp'&&(!data.host?.trim()||!Number.isInteger(Number(data.port))||Number(data.port)<1||Number(data.port)>65535))return '호스트와 1~65535 범위의 포트를 입력하세요.';
    if(data.checkType==='process'&&!processes.some(process=>process.name===data.processName))return '확인할 프로세스를 선택하세요.';
  }
  if(data.kind==='condition'&&data.source==='file_exists'&&!data.path.trim())return '확인할 파일 경로를 입력하세요.';
  if(data.kind==='condition'&&data.source==='exit_code'&&!/^-?\d+$/.test(data.value))return '종료 코드에는 정수를 입력하세요.';
  return '';
}

export function workflowIssues(workflow,ownerId,processes=[]) {
  const issues=[],start=workflow.nodes.find(node=>node.data.kind==='trigger');
  const push=(node,message)=>issues.push({id:node?.id,message});
  if(!start)push(null,'시작 단계가 필요합니다.');
  if(!workflow.nodes.some(node=>node.data.kind==='finish'))push(null,'종료 단계를 연결하세요.');
  const reachable=new Set(),pending=start?[start.id]:[];
  while(pending.length){const id=pending.pop();if(reachable.has(id))continue;reachable.add(id);pending.push(...workflow.edges.filter(edge=>edge.source===id).map(edge=>edge.target));}
  for(const node of workflow.nodes){const {data}=node,exits=workflow.edges.filter(edge=>edge.source===node.id);
    if(!reachable.has(node.id))push(node,`${data.label}: 시작에서 이어지는 연결이 없습니다.`);
    if(!data.label?.trim())push(node,'단계 이름을 입력하세요.');
    if(data.kind==='condition'){
      if(!exits.some(edge=>edge.sourceHandle==='true')||!exits.some(edge=>edge.sourceHandle==='false'))push(node,`${data.label}: 참과 거짓 경로를 모두 연결하세요.`);
    }else if(resultRoutes(data).length||data.kind==='parallel'){
      for(const port of sourcePorts(data))if(!exits.some(edge=>(edge.sourceHandle||'out')===port.id))push(node,`${data.label}: ${port.label} 경로를 연결하세요.`);
    }else if(data.kind!=='finish'&&!exits.length)push(node,`${data.label}: 다음 단계를 연결하세요.`);
    const executionError=stepExecutionError(data,ownerId,processes);if(executionError)push(node,executionError);
  }
  for(const edge of workflow.edges){const error=stepConnectionError(workflow,edge,edge.id);if(error)push(workflow.nodes.find(node=>node.id===edge.source),error);}
  return [...issues,...parallelIssues(workflow)];
}

export function stepSize(data) {
  const count=data.kind==='parallel'?(data.branches||[]).length:resultRoutes(data).length;
  return count?{width:260,height:110+(count+(data.kind==='parallel'?0:1))*30}:{width:220,height:112};
}

export function workflowGroupFrame(group,nodes) {
  const members=nodes.filter(node=>group.nodeIds.includes(node.id));
  const x=Math.min(...members.map(node=>node.position.x))-24,y=Math.min(...members.map(node=>node.position.y))-58;
  return {position:{x,y},width:Math.max(...members.map(node=>node.position.x+stepSize(node.data).width))+24-x,height:Math.max(...members.map(node=>node.position.y+stepSize(node.data).height))+24-y};
}

export function arrangeWorkflow(workflow) {
  const depths=new Map(),seen=new Set();
  const depth=id=>{if(depths.has(id))return depths.get(id);if(seen.has(id))return 0;seen.add(id);const incoming=workflow.edges.filter(edge=>edge.target===id);const rank=incoming.length?1+Math.max(...incoming.map(edge=>depth(edge.source))):0;seen.delete(id);depths.set(id,rank);return rank;};
  workflow.nodes.forEach(node=>depth(node.id));
  const layers=new Map();for(const node of workflow.nodes){const rank=depths.get(node.id);if(!layers.has(rank))layers.set(rank,[]);layers.get(rank).push(node);}
  const positions=new Map();
  let y=0;
  for(const [,layer] of [...layers].sort(([left],[right])=>left-right)){
    const branchOrder=node=>{
      const incoming=workflow.edges.filter(edge=>edge.target===node.id);
      if(!incoming.length)return 0;
      return incoming.reduce((sum,edge)=>sum+(positions.get(edge.source)?.x||0)+(edge.sourceHandle==='true'?-100:edge.sourceHandle==='false'?100:0),0)/incoming.length;
    };
    layer.sort((left,right)=>branchOrder(left)-branchOrder(right));
    layer.forEach((node,index)=>positions.set(node.id,{x:150+(index-(layer.length-1)/2)*300,y}));
    y+=Math.max(...layer.map(node=>stepSize(node.data).height))+48;
  }
  return {...workflow,nodes:workflow.nodes.map(node=>({...node,position:positions.get(node.id)}))};
}

export function newStep(kind) {
  const defaults={command:{command:'',cwd:'',environment:{},timeoutSeconds:''},condition:{source:'exit_code',operator:'equals',value:'0',path:''},wait:{seconds:'30'},process:{processName:'',operation:'restart'},health:{checkType:'http',url:'http://127.0.0.1:3000/health',expectedStatus:'200',host:'127.0.0.1',port:'3000',processName:'',timeoutSeconds:'30'},job:{jobId:''},finish:{result:'succeeded',message:''}};
  return {id:crypto.randomUUID(),type:'step',position:{x:0,y:0},data:{kind,label:stepKinds[kind].label,...defaults[kind]}};
}
