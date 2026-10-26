export function createParallelSegment(){
  const id=crypto.randomUUID(),joinId=crypto.randomUUID();
  const branches=[{id:`branch-${crypto.randomUUID()}`,label:'분기 1'},{id:`branch-${crypto.randomUUID()}`,label:'분기 2'}];
  return {nodes:[{id,type:'step',position:{x:0,y:0},data:{kind:'parallel',label:'병렬 실행',branches,joinId}},{id:joinId,type:'step',position:{x:0,y:0},data:{kind:'join',label:'결과 합류',parallelId:id,joinMode:'all',failurePolicy:'fail'}}],edges:branches.map(branch=>({id:crypto.randomUUID(),source:id,sourceHandle:branch.id,target:joinId,targetHandle:'in'}))};
}
export function parallelIssues(workflow){
  const issues=[];
  for(const node of workflow.nodes.filter(node=>node.data.kind==='parallel')){
    const join=workflow.nodes.find(item=>item.id===node.data.joinId&&item.data.kind==='join');
    if(!join||join.data.parallelId!==node.id){issues.push({id:node.id,message:`${node.data.label}: 짝이 맞는 합류 단계가 필요합니다.`});continue;}
    if((node.data.branches||[]).length<2)issues.push({id:node.id,message:`${node.data.label}: 병렬 분기는 2개 이상이어야 합니다.`});
    const used=new Set(),branchIds=new Set();
    for(const branch of node.data.branches||[]){if(!branch.id||branchIds.has(branch.id)||!branch.label?.trim())issues.push({id:node.id,message:'병렬 분기의 이름과 고유한 연결점이 필요합니다.'});branchIds.add(branch.id);}
    for(const branch of node.data.branches||[]){
      const edge=workflow.edges.find(edge=>edge.source===node.id&&edge.sourceHandle===branch.id);
      if(!edge){issues.push({id:node.id,message:`${branch.label}: 첫 단계를 연결하세요.`});continue;}
      const region=new Set(),pending=[edge.target];let invalid=false;
      while(pending.length){const id=pending.pop();if(id===join.id||region.has(id))continue;region.add(id);const step=workflow.nodes.find(item=>item.id===id),out=workflow.edges.filter(item=>item.source===id);if(!step||!out.length||step.data.kind==='finish'){invalid=true;continue;}pending.push(...out.map(item=>item.target));}
      if(invalid)issues.push({id:node.id,message:`${branch.label}: 모든 경로를 ${join.data.label}에 연결하세요.`});
      if([...region].some(id=>used.has(id)))issues.push({id:node.id,message:`${node.data.label}: 분기 사이의 공통 단계는 합류 뒤에 배치하세요.`});
      region.forEach(id=>used.add(id));
    }
  }
  for(const node of workflow.nodes.filter(node=>node.data.kind==='join'))if(!workflow.nodes.some(item=>item.id===node.data.parallelId&&item.data.kind==='parallel'))issues.push({id:node.id,message:`${node.data.label}: 병렬 실행 단계가 없습니다.`});
  return issues;
}
