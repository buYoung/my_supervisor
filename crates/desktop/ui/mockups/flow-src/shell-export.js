import {initialWorkflow,workflowIssues} from './workflow-model.js';
import {resultRoutes} from './result-routing.js';
import {shellRuntime} from './shell-runtime.js';

const quote=value=>`'${String(value??'').replaceAll("'","'\\''")}'`;
const integer=(value,fallback=0)=>Number.isFinite(Number(value))?Math.max(0,Math.trunc(Number(value)||fallback)):fallback;
function fingerprint(value){let hash=2166136261;for(const char of value)hash=Math.imul(hash^char.charCodeAt(0),16777619);return (hash>>>0).toString(16);}
const assignment=(key,value)=>`${key}=${quote(value)}`;
const branchId=index=>`b${index}`;
const configDefaults={MSV_KIND:'',MSV_LABEL:'',MSV_TIMEOUT:0,MSV_ATTEMPTS:1,MSV_DELAY:1,MSV_STRATEGY:'fixed',MSV_INCREMENT:1,MSV_MULTIPLIER:2,MSV_MAX_DELAY:60,MSV_JITTER:0,MSV_RETRY_ON:'failure',MSV_CIRCUIT:0,MSV_CIRCUIT_KEY:'',MSV_THRESHOLD:3,MSV_OPEN_SECONDS:30,MSV_RECOVERY:1,MSV_PROBE_LEASE:300,MSV_HAS_ROUTES:0,MSV_ON_ERROR:'stop',MSV_SOURCE:'',MSV_OPERATOR:'equals',MSV_VALUE:'0',MSV_PATH:'',MSV_BRANCHES:'',MSV_JOIN:'',MSV_JOIN_MODE:'all',MSV_FAILURE_POLICY:'fail'};

export function createShellExport({jobs,rootIds,group=null,processes=[],shell='bash',includeEnvironment=false,sourceLabel=''}){
  const errors=[],included=[],visited=new Set(),byId=new Map(jobs.map(job=>[job.id,job]));
  function include(id,ancestors=[]){
    if(ancestors.includes(id)){errors.push('다른 작업 호출에 순환 참조가 있습니다.');return;}
    if(visited.has(id))return;
    const job=byId.get(id);if(!job){errors.push('호출 대상 작업을 찾을 수 없습니다.');return;}
    visited.add(id);included.push(job);
    const graph=initialWorkflow(job);
    for(const issue of workflowIssues(graph,id,processes))errors.push(`${job.name}: ${issue.message}`);
    for(const node of graph.nodes)if(node.data.kind==='job')include(node.data.jobId,[...ancestors,id]);
  }
  rootIds.forEach(id=>include(id));
  const aliases=new Map(included.map((job,index)=>[job.id,`j${index}`]));
  const nodes=new Map(included.map(job=>[job.id,new Map(initialWorkflow(job).nodes.map((node,index)=>[node.id,`n${index}`]))]));
  const required=new Set(['awk','ps','date','mktemp','sleep','cat','mkdir','mv','rm','rmdir']);
  const config=[],actions=[],edges=[],routes=[],starts=[],parents=[],dependency=[],kindCases=[],plan=[];
  let environmentCount=0,stepCount=0,hasCircuit=false;
  for(const job of included){
    const id=aliases.get(job.id),graph=initialWorkflow(job),nodeMap=nodes.get(job.id);
    starts.push(`${id}) printf '%s\\n' ${quote(nodeMap.get(graph.nodes.find(node=>node.data.kind==='trigger')?.id))} ;;`);
    parents.push(`${id}) printf '%s\\n' ${quote(group?(job.upstream||[]).filter(upstream=>rootIds.includes(upstream)).map(upstream=>aliases.get(upstream)).join(' '):'')} ;;`);
    dependency.push(`${id}) printf '%s\\n' ${quote(job.onDependencyFailure||'skip')} ;;`);
    plan.push(`printf '%s\\n' ${quote(`${job.name} · ${graph.nodes.length}개 단계${rootIds.includes(job.id)?'':' · 호출 대상'}`)}`);
    for(const edge of graph.edges){
      const source=graph.nodes.find(node=>node.id===edge.source),port=source?.data.kind==='parallel'?branchId(source.data.branches.findIndex(branch=>branch.id===edge.sourceHandle)):edge.sourceHandle||'out';
      edges.push(`${quote(`${id}:${nodeMap.get(edge.source)}:${port}`)}) printf '%s\\n' ${quote(nodeMap.get(edge.target))} ;;`);
    }
    for(const node of graph.nodes){
      stepCount++;
      const key=`${id}:${nodeMap.get(node.id)}`,data=node.data,circuit=data.circuitBreaker||{},nodeRoutes=resultRoutes(data),env=Object.entries(data.environment||{});
      environmentCount+=env.length;hasCircuit ||=Boolean(circuit.enabled);
      if(env.some(([key])=>!/^[_A-Za-z][_A-Za-z0-9]*$/.test(key)))errors.push(`${data.label}: 셸 환경변수 이름으로 사용할 수 없는 키가 있습니다.`);
      const fields={
        MSV_KIND:data.kind,MSV_LABEL:`${job.name} / ${data.label}`,MSV_TIMEOUT:integer(data.timeoutSeconds),MSV_ATTEMPTS:integer(data.maxAttempts,1),
        MSV_DELAY:Number(data.retryDelaySeconds)||1,MSV_STRATEGY:data.retryStrategy||'fixed',MSV_INCREMENT:Number(data.retryIncrementSeconds)||1,
        MSV_MULTIPLIER:Number(data.retryMultiplier)||2,MSV_MAX_DELAY:Number(data.retryMaxDelaySeconds)||60,MSV_JITTER:data.retryJitter?1:0,MSV_RETRY_ON:data.retryOn||'failure',
        MSV_CIRCUIT:circuit.enabled?1:0,MSV_CIRCUIT_KEY:circuit.enabled?fingerprint(`${job.id}:${node.id}`):'',MSV_THRESHOLD:integer(circuit.failureThreshold,3),MSV_OPEN_SECONDS:integer(circuit.openSeconds,30),MSV_RECOVERY:integer(circuit.recoverySuccesses,1),
        MSV_PROBE_LEASE:Math.max(300,integer(data.timeoutSeconds)*integer(data.maxAttempts,1)+integer(data.retryMaxDelaySeconds,60)*integer(data.maxAttempts,1)+60),
        MSV_HAS_ROUTES:nodeRoutes.length?1:0,MSV_ON_ERROR:data.onError||'stop',MSV_SOURCE:data.source||'',MSV_OPERATOR:data.operator||'equals',MSV_VALUE:data.value||'0',MSV_PATH:data.path||'',
        MSV_BRANCHES:(data.branches||[]).map((_,index)=>branchId(index)).join(' '),MSV_JOIN:nodeMap.get(data.joinId)||'',MSV_JOIN_MODE:data.joinMode||'all',MSV_FAILURE_POLICY:data.failurePolicy||'fail',
      };
      config.push(`${quote(key)})\n      ${Object.entries(fields).filter(([key,value])=>String(value)!==String(configDefaults[key])).map(([key,value])=>assignment(key,value)).join('\n      ')}\n      ;;`);
      kindCases.push(`${quote(key)}) printf '%s\\n' ${quote(data.kind==='condition'&&data.source==='exit_code'?'condition_exit':data.kind)} ;;`);
      const routeLines=nodeRoutes.map(route=>`if msv_match ${quote(route.source)} ${quote(route.operator)} ${quote(route.value)} "$3" "$4" "$5"; then printf '%s\\n' ${quote(route.id)}; return; fi`);
      if(routeLines.length)routes.push(`${quote(key)})\n      ${routeLines.join('\n      ')}\n      ;;`);
      const code=[];
      if(data.kind==='command'){
        if(data.cwd)code.push(`cd -- ${quote(data.cwd)} || return 1`);
        for(const [key] of env)if(!includeEnvironment&&/^[_A-Za-z][_A-Za-z0-9]*$/.test(key))code.push(`: "\${${key}:?${key} 환경변수를 설정하세요}"`);
        const environment=env.map(([key,value])=>includeEnvironment?quote(`${key}=${value}`):`${quote(`${key}=`)}"$${key}"`);
        if(env.length)required.add('env');
        code.push(`${env.length?`env ${environment.join(' ')} `:''}"$MSV_SHELL" -c ${quote(data.command)}; return $?`);
      }else if(data.kind==='process'){
        required.add('msv');code.push(`"$MSV_CLI" ${quote(data.operation)} ${quote(data.processName)}; return $?`);
      }else if(data.kind==='health'){
        if(data.checkType==='http'){
          required.add('curl');code.push(`local http_status; http_status=$(curl --silent --show-error --output /dev/null --write-out '%{http_code}' --max-time ${integer(data.timeoutSeconds,30)} -- ${quote(data.url)}) || return $?`,`[ "$http_status" = ${quote(data.expectedStatus||200)} ]`);
        }else if(data.checkType==='tcp'){
          required.add('nc');code.push(`nc -z -w ${integer(data.timeoutSeconds,5)} -- ${quote(data.host)} ${quote(data.port)}`);
        }else{
          required.add('msv');required.add('jq');code.push(`"$MSV_CLI" -o json show ${quote(data.processName)} | jq -e '.state == "running"' >/dev/null`);
        }
      }else if(data.kind==='wait')code.push(`sleep ${integer(data.seconds,1)}`);
      else if(data.kind==='job')code.push(`msv_path ${quote(aliases.get(data.jobId))} "$(msv_start ${quote(aliases.get(data.jobId))})" '' 0 "$4" "$5"; return $?`);
      else if(data.kind==='finish'){if(data.message)code.push(`printf '%s\\n' ${quote(data.message)}`);code.push(`return ${data.result==='failed'?1:0}`);}
      else code.push('return 0');
      actions.push(`# ${job.name.replace(/[\r\n]/g,' ')} / ${data.label.replace(/[\r\n]/g,' ')}\n    ${quote(key)})\n      ${code.join('\n      ')}\n      ;;`);
      plan.push(`printf '%s\\n' ${quote(`  ${data.label} [${data.kind}]${data.kind==='command'?` $ ${data.command}`:data.kind==='join'?` · ${data.joinMode||'all'}`:''}`)}`);
    }
  }
  const title=group?.name||included[0]?.name||'workflow',namespace=fingerprint(rootIds.join(':'));
  const roots=rootIds.map(id=>aliases.get(id)).filter(Boolean).join(' ');
  const caseFunction=(name,selector,cases,fallback='return 1',after='',before='')=>`${name}() {\n${before?`  ${before}\n`:''}  case "${selector}" in\n    ${cases.join('\n    ')}\n    *) ${fallback} ;;\n  esac\n${after?`  ${after}\n`:''}}\n`;
  const header=[`#!/usr/bin/env ${shell}`,shell==='zsh'?'emulate -R sh':'',
    '# my_supervisor · 흐름 내보내기',`# ${title.replaceAll('\n',' ')}`,`# ${sourceLabel.replaceAll('\n',' ')}`,
    '# 실행 동작과 의존 관계를 한 번 수행합니다. 예약·자동 시작은 등록하지 않습니다.',
    '# --dry-run: 외부 동작 없이 포함된 단계 확인',
    '# MSV_CLI: msv 실행 파일 | MSV_STATE_DIR: 서킷 상태 저장 경로',
    '# 취소는 실행 중인 자식 프로세스를 종료합니다. 완료된 외부 동작은 되돌리지 않습니다.',
    'umask 077',`if [ -z "$MSV_SHELL" ]; then MSV_SHELL=${quote(shell==='zsh'?'/bin/zsh':'/bin/bash')}; fi`,
    'if [ -z "$MSV_CLI" ]; then MSV_CLI=msv; fi',
    `if [ -z "$MSV_STATE_DIR" ]; then MSV_STATE_DIR="$HOME/.local/state/my-supervisor/exports/${namespace}"; fi`,
    'if [ -z "$TMPDIR" ]; then TMPDIR=/tmp; fi',
    assignment('MSV_ROOTS',roots),assignment('MSV_MAX_PARALLEL',group?integer(group.policy?.maxParallel,4):1),
    assignment('MSV_GROUP_FAILURE',group?.policy?.failurePolicy||'continue'),assignment('MSV_GROUP_TIMEOUT',integer(group?.policy?.timeoutSeconds)),
    assignment('MSV_REQUIREMENTS',[...required].join(' ')),"MSV_TMP=''; MSV_MAIN_PID=''; MSV_TIMER_PID=''",'',
  ].filter((line,index)=>line||index>1).join('\n');
  const script=header+'\n'+caseFunction('msv_action','$1:$2',actions)+caseFunction('msv_config','$1:$2',config,'return 1','',Object.entries(configDefaults).map(([key,value])=>assignment(key,value)).join('\n  '))+caseFunction('msv_edge','$1:$2:$3',edges)+caseFunction('msv_route','$1:$2',routes,':',"printf '%s\\n' out");
  const finalScript=script+
    caseFunction('msv_start','$1',starts)+caseFunction('msv_parents','$1',parents)+caseFunction('msv_dependency_policy','$1',dependency)+caseFunction('msv_kind','$1:$2',kindCases)+
    `msv_plan() {\n  printf '%s\\n' ${quote(`${title} · ${shell} · ${sourceLabel}`)}\n  ${plan.join('\n  ')}\n}\n`+
    shellRuntime+'\nmsv_main "$1"\nexit $?\n';
  const warnings=['예약 일정은 등록하지 않습니다. 수동 실행 또는 외부 스케줄러에서 호출하세요.'];
  if(environmentCount)warnings.push(includeEnvironment?'환경변수 값이 파일에 포함됩니다.':'환경변수 값은 제외했습니다. 실행 환경에 같은 이름으로 설정해야 합니다.');
  if(hasCircuit)warnings.push('서킷 상태는 MSV_STATE_DIR에 유지합니다. 파일을 지우면 상태가 초기화됩니다.');
  if(required.has('msv'))warnings.push('프로세스 제어는 실행 환경의 msv CLI와 연결된 데몬을 사용합니다.');
  return {script:errors.length?'':finalScript,errors:[...new Set(errors)],warnings,requirements:[...required].filter(name=>!['awk','ps','date','mktemp','sleep','cat','mkdir','mv','rm','rmdir','env'].includes(name)),filename:`${title.replace(/[^a-zA-Z0-9가-힣._-]+/g,'-')}.${shell==='zsh'?'zsh':'sh'}`,stepCount,jobCount:included.length,environmentCount};
}
