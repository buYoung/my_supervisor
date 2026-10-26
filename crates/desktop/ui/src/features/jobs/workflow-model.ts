import type {FlowEdge, FlowNode, StepData, StepKind, Workflow} from "../../services/workspace-types";

export const stepKinds: Array<{kind:StepKind;label:string;description:string}> = [
  {kind:"command",label:"명령 실행",description:"프로그램과 스크립트 실행"},
  {kind:"process",label:"프로세스 제어",description:"등록한 프로세스 시작 · 중지 · 재시작"},
  {kind:"condition",label:"조건 분기",description:"종료 코드 · 출력 · 파일 조건"},
  {kind:"health",label:"상태 확인",description:"HTTP · TCP 연결 확인"},
  {kind:"wait",label:"대기",description:"다음 단계를 일정 시간 지연"},
  {kind:"parallel",label:"병렬 분기",description:"여러 경로를 동시에 시작"},
  {kind:"join",label:"합류",description:"전체 완료 · 첫 성공 · race"},
  {kind:"job",label:"다른 작업 실행",description:"등록한 작업의 흐름 호출"},
  {kind:"finish",label:"종료",description:"성공 또는 실패로 마무리"},
];
export function outputs(data:StepData):Array<{id:string;label:string}> {
  if(data.kind==="finish")return [];
  if(data.kind==="condition")return [{id:"true",label:"참"},{id:"false",label:"거짓"}];
  if(data.kind==="parallel")return Array.from({length:Number(data.branchCount||2)},(_,i)=>({id:`branch-${i+1}`,label:`경로 ${i+1}`}));
  return [...(data.resultRoutes||[]).map(route=>({id:route.id,label:route.label})),{id:"out",label:(data.resultRoutes||[]).length?"그 외":"다음"}];
}
export function newStep(kind:StepKind,position={x:320,y:220}):FlowNode {
  const label=stepKinds.find(item=>item.kind===kind)?.label||"시작";
  const data:StepData={kind,label,maxAttempts:1,timeoutSeconds:60};
  if(kind==="command")Object.assign(data,{program:"/bin/sh",args:["-c","echo hello"]});
  if(kind==="process")data.operation="restart";
  if(kind==="condition")Object.assign(data,{source:"exit_code",operator:"equals",value:"0"});
  if(kind==="health")Object.assign(data,{checkType:"http",url:"http://127.0.0.1:3000/health",expectedStatus:200});
  if(kind==="wait")data.seconds=5;
  if(kind==="parallel")data.branchCount=2;
  if(kind==="join")Object.assign(data,{joinMode:"all",failurePolicy:"fail"});
  if(kind==="finish")data.result="succeeded";
  return {id:crypto.randomUUID(),position,data};
}
export function canConnect(flow:Workflow,source:string,target:string,handle:string):string|null {
  if(source===target)return "자기 자신과 연결할 수 없습니다.";
  if(flow.nodes.find(node=>node.id===target)?.data.kind==="trigger")return "시작 단계로 연결할 수 없습니다.";
  if(flow.edges.some(edge=>edge.source===source&&edge.sourceHandle===handle))return "이미 연결된 출력입니다. 기존 연결을 지운 뒤 연결하세요.";
  const visit=(id:string,seen=new Set<string>()):boolean=>{if(id===source)return true;if(seen.has(id))return false;seen.add(id);return flow.edges.filter(edge=>edge.source===id).some(edge=>visit(edge.target,seen));};
  return visit(target)?"순환 연결은 만들 수 없습니다.":null;
}
export function edge(source:string,target:string,sourceHandle="out"):FlowEdge{return {id:crypto.randomUUID(),source,target,sourceHandle};}
export function flowDiff(before:Workflow,after:Workflow) {
  const nodes=new Map<string,"added"|"removed"|"modified">(),edges=new Map<string,"added"|"removed"|"modified">();
  for(const node of after.nodes){const old=before.nodes.find(item=>item.id===node.id);if(!old)nodes.set(node.id,"added");else if(JSON.stringify(old.data)!==JSON.stringify(node.data))nodes.set(node.id,"modified");}
  for(const node of before.nodes)if(!after.nodes.some(item=>item.id===node.id))nodes.set(node.id,"removed");
  for(const item of after.edges){const old=before.edges.find(e=>e.id===item.id);if(!old)edges.set(item.id,"added");else if(JSON.stringify(old)!==JSON.stringify(item))edges.set(item.id,"modified");}
  for(const item of before.edges)if(!after.edges.some(e=>e.id===item.id))edges.set(item.id,"removed");
  return {nodes,edges};
}
export const stateLabels:Record<string,string>={pending:"대기",running:"실행 중",succeeded:"성공",failed:"실패",timed_out:"시간 초과",cancelled:"취소",skipped:"건너뜀",circuit_open:"회로 열림"};
