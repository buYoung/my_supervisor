import {arrangeWorkflow,initialWorkflow,stepSize} from './workflow-model.js';
import {resultRoutes,sourcePorts,supportsResultRoutes} from './result-routing.js';

const canonical=value=>Array.isArray(value)?value.map(canonical):value&&typeof value==='object'?Object.fromEntries(Object.keys(value).sort().filter(key=>value[key]!==undefined).map(key=>[key,canonical(value[key])])):value;
const equal=(before,after)=>JSON.stringify(canonical(before))===JSON.stringify(canonical(after));
const fieldNames={workflowGroupId:'소속 그룹',groupPolicy:'그룹 실행 정책',retryStrategy:'재시도 방식',retryIncrementSeconds:'증가 간격 (초)',retryMultiplier:'증가 배수',retryMaxDelaySeconds:'최대 간격 (초)',retryJitter:'간격 분산',retryOn:'재시도 대상',circuitBreaker:'서킷 브레이커',failureThreshold:'실패 임계값',openSeconds:'차단 시간 (초)',recoverySuccesses:'복구 확인 횟수',onError:'최종 실패 처리',branches:'병렬 분기',joinMode:'합류 방식',failurePolicy:'실패 정책',resultRoutes:'결과별 경로',name:'작업 이름',label:'단계 이름',kind:'단계 유형',command:'실행 명령',cwd:'작업 디렉터리',environment:'환경변수',executable:'실행자',triggerType:'시작 방식',triggerValue:'실행 일정·선행 작업',timezone:'시간대',policy:'중복 실행 정책',onDependencyFailure:'선행 작업 실패 처리',connectionGroupName:'연결 그룹 이름',groupName:'그룹 이름',timeoutSeconds:'제한 시간 (초)',maxAttempts:'총 시도 횟수',retryDelaySeconds:'재시도 간격 (초)',processName:'대상 프로세스',operation:'제어 동작',checkType:'확인 방식',url:'확인 URL',expectedStatus:'HTTP 상태 코드',host:'호스트',port:'포트',seconds:'대기 시간 (초)',source:'조건 대상',operator:'비교 방식',value:'조건 값',path:'파일 경로',result:'종료 결과',message:'결과 메시지',jobId:'호출할 작업',position:'배치 위치',sourceHandle:'출발 포트',targetHandle:'도착 포트',sourceLabel:'출발 단계',targetLabel:'도착 단계'};
const labelFor=path=>path.split('.').map(part=>fieldNames[part]||part).join(' · ');
function flatten(value,prefix='',result={}) {
  for(const [key,item] of Object.entries(value||{})) {
    const path=prefix?`${prefix}.${key}`:key;
    if(!prefix&&key==='resultRoutes')continue;
    if(item&&typeof item==='object'&&!Array.isArray(item))flatten(item,path,result);
    else result[path]=item;
  }
  return result;
}
export function configurationDiff(before,after) {
  const left=flatten(before),right=flatten(after);
  const fields=[...new Set([...Object.keys(left),...Object.keys(right)])].filter(key=>!equal(left[key],right[key])).map(key=>({key,label:labelFor(key),before:left[key],after:right[key],secret:key.startsWith('environment.'),status:left[key]===undefined?'added':right[key]===undefined?'removed':'modified'}));
  const oldRoutes=new Map((before?.resultRoutes||[]).map(route=>[route.id,route])),newRoutes=new Map((after?.resultRoutes||[]).map(route=>[route.id,route]));
  const routeFields={label:'경로 이름',source:'결과 종류',operator:'비교 방식',value:'비교 값'};
  for(const id of new Set([...oldRoutes.keys(),...newRoutes.keys()])) {
    const previous=oldRoutes.get(id),next=newRoutes.get(id),label=(next||previous).label;
    for(const [key,title] of Object.entries(routeFields))if(!equal(previous?.[key],next?.[key]))fields.push({key:`resultRoutes.${id}.${key}`,label:`결과별 경로 · ${label} · ${title}`,before:previous?.[key],after:next?.[key],status:!previous?'added':!next?'removed':'modified'});
  }
  const oldOrder=[...oldRoutes.keys()].filter(id=>newRoutes.has(id)),newOrder=[...newRoutes.keys()].filter(id=>oldRoutes.has(id));
  if(!equal(oldOrder,newOrder))fields.push({key:'resultRoutes.order',label:'결과별 경로 · 확인 순서',before:oldOrder.map(id=>oldRoutes.get(id).label).join(' → '),after:newOrder.map(id=>newRoutes.get(id).label).join(' → '),status:'modified'});
  return fields;
}
const edgeKey=edge=>JSON.stringify([edge.source,edge.sourceHandle||'out',edge.target,edge.targetHandle||'in']);
const portKey=edge=>JSON.stringify([edge.source,edge.sourceHandle||'out']);
const edgeSettings=(edge,nodes)=>edge?{sourceLabel:nodes.get(edge.source)?.data.label||edge.source,sourceHandle:sourcePorts(nodes.get(edge.source)?.data||{}).find(port=>port.id===(edge.sourceHandle||'out'))?.label||edge.sourceHandle||'out',targetLabel:nodes.get(edge.target)?.data.label||edge.target,targetHandle:edge.targetHandle||'in',...(edge.label?{label:edge.label}:{}),...(edge.data?{data:edge.data}:{})}:undefined;

export function compareVersions(before,after,jobId) {
  const oldGraph=before?initialWorkflow({...before.snapshot,id:jobId}):{nodes:[],edges:[]};
  const newGraph=after?initialWorkflow({...after.snapshot,id:jobId}):{nodes:[],edges:[]};
  const oldNodes=new Map(oldGraph.nodes.map(node=>[node.id,node])),newNodes=new Map(newGraph.nodes.map(node=>[node.id,node]));
  const nodeChanges=[...new Set([...newNodes.keys(),...oldNodes.keys()])].map(id=>{
    const previous=oldNodes.get(id),next=newNodes.get(id);
    const fields=configurationDiff(previous?.data,next?.data),settingsChanged=fields.length>0;
    const moved=Boolean(previous&&next&&!equal(previous.position,next.position));
    if(moved)fields.push({key:'position',label:'배치 위치',before:previous.position,after:next.position,status:'modified'});
    return {id,label:(next||previous).data.label,before:previous,after:next,fields,status:!previous?'added':!next?'removed':settingsChanged?'modified':moved?'moved':'unchanged'};
  });
  let hasRemovedRoutes=false;
  const nodes=nodeChanges.map(change=>{
    const node={...structuredClone(change.after||change.before),comparisonStatus:change.status};
    if(supportsResultRoutes(node.data)) {
      const previousRoutes=resultRoutes(change.before?.data||{}),nextRoutes=resultRoutes(change.after?.data||{});
      const deleted=previousRoutes.filter(route=>!nextRoutes.some(item=>item.id===route.id));
      if(deleted.length)hasRemovedRoutes=true;
      node.data.resultRoutes=[...nextRoutes.map(route=>({...route,changeStatus:previousRoutes.some(item=>item.id===route.id)?undefined:'added'})),...deleted.map(route=>({...route,changeStatus:'removed'}))];
    }
    return node;
  });
  // Keep the target layout; place a deleted step beside any overlapping replacement.
  const occupied=nodes.filter(node=>newNodes.has(node.id));
  for(const node of nodes.filter(node=>!newNodes.has(node.id))) {
    while(occupied.some(other=>node.position.x<other.position.x+stepSize(other.data).width+20&&node.position.x+stepSize(node.data).width+20>other.position.x&&node.position.y<other.position.y+stepSize(other.data).height+13&&node.position.y+stepSize(node.data).height+13>other.position.y))node.position={x:node.position.x+300,y:node.position.y};
    occupied.push(node);
  }
  const remaining=[...newGraph.edges],edgeChanges=[],edges=[];
  const addEdgeChange=(previous,next,status)=>{
    const id=`comparison-edge-${edgeChanges.length}`;
    const left=edgeSettings(previous,oldNodes),right=edgeSettings(next,newNodes);
    const change={id,before:previous,after:next,status,fields:configurationDiff(left,right),label:`${(right||left).sourceLabel} → ${(right||left).targetLabel}`};
    edgeChanges.push(change);
    const routeChanged=previous&&next&&edgeKey(previous)!==edgeKey(next);
    if(routeChanged)edges.push({...structuredClone(previous),id:`${id}-before`,comparisonId:id,comparisonStatus:status,comparisonSide:'before'});
    edges.push({...structuredClone(next||previous),id,comparisonId:id,comparisonStatus:status,comparisonSide:next?'after':'before'});
  };
  // Match identical routes first, then pair a changed destination on the same port.
  const unmatched=[];
  for(const previous of oldGraph.edges) {
    const index=remaining.findIndex(edge=>edgeKey(edge)===edgeKey(previous));
    if(index<0){unmatched.push(previous);continue;}
    const [next]=remaining.splice(index,1);
    const settings=edge=>({label:edge.label,data:edge.data});
    addEdgeChange(previous,next,equal(settings(previous),settings(next))?'unchanged':'modified');
  }
  for(const previous of unmatched) {
    const index=remaining.findIndex(edge=>portKey(edge)===portKey(previous));
    if(index<0)addEdgeChange(previous,null,'removed');
    else addEdgeChange(previous,remaining.splice(index,1)[0],'modified');
  }
  remaining.forEach(next=>addEdgeChange(null,next,'added'));
  const jobSettings=version=>{const {workflow,...settings}=version?.snapshot||{};return settings;};
  const jobFields=configurationDiff(jobSettings(before),jobSettings(after));
  const counts=list=>Object.fromEntries(['added','removed','modified'].map(status=>[status,list.filter(item=>item.status===status).length]));
  const graph=hasRemovedRoutes?arrangeWorkflow({nodes,edges}):{nodes,edges};
  return {graph,nodeChanges,edgeChanges,jobFields,nodeCounts:counts(nodeChanges),edgeCounts:counts(edgeChanges)};
}
