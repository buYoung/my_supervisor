(() => {
  'use strict';
  const labels={pending:'대기',running:'실행 중',succeeded:'완료',failed:'실패',skipped:'건너뜀',cancelled:'취소'};
  const escape=value=>String(value??'').replace(/[&<>"']/g,char=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));
  const duration=value=>value==null?'—':value<1000?`${Math.round(value)}ms`:`${(value/1000).toFixed(1)}초`;
  const date=value=>new Date(value).toLocaleString('ko-KR',{timeZone:'Asia/Seoul',month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit',second:'2-digit',hour12:false});

  function list(job){
    const id=job.id||job.flowId||job.name;
    if(window.SupervisorWorkflowRuns)return window.SupervisorWorkflowRuns.list(id);
    try {const history=JSON.parse(sessionStorage.getItem(`msv-mock-workflow-history-${id}`));if(history?.length)return history;const last=JSON.parse(sessionStorage.getItem(`msv-mock-workflow-run-${id}`));return last?[last]:[];}catch{return [];}
  }

  function path(run){
    if(!run?.workflow)return [];
    const nodes=new Map(run.workflow.nodes.map(node=>[node.id,node])),visited=new Set(run.visitedEdges||[]),seen=new Set();
    function walk(id,stop){
      const result=[];
      while(id&&id!==stop&&!seen.has(id)){
        const node=nodes.get(id),state=run.steps?.[id];
        if(!node||!state||state.state==='pending'||state.state==='skipped')break;
        seen.add(id);
        const outgoing=run.workflow.edges.filter(edge=>edge.source===id&&visited.has(edge.id));
        const item={id,label:node.data.label,state:state.state,decision:state.selectedRoute?.label||(state.branch==='true'?'참':state.branch==='false'?'거짓':''),isFailureHandled:state.isFailureHandled};
        if(node.data.kind==='parallel'){
          const outcomes=state.branchResults||[];
          result.push({...item,kind:'parallel',branches:(node.data.branches||[]).map(branch=>{const edge=outgoing.find(edge=>edge.sourceHandle===branch.id),outcome=outcomes.find(item=>item.id===branch.id);return {...branch,state:outcome?.state,isWinner:outcome?.isWinner,steps:edge?walk(edge.target,node.data.joinId):[]};})});
          id=node.data.joinId;
        }else{result.push({...item,kind:'node'});id=outgoing[0]?.target;}
      }
      return result;
    }
    const first=run.scope==='step'?run.workflow.nodes.find(node=>run.steps?.[node.id]?.startedAt):run.workflow.nodes.find(node=>node.data.kind==='trigger');
    return walk(first?.id);
  }

  function summary(run){
    const steps=Object.values(run.steps||{}),executed=steps.filter(step=>step.startedAt||!['pending','skipped'].includes(step.state));
    const failureNode=steps.find(step=>step.id===run.failedNodeId);
    const failed=(failureNode?.kind!=='finish'?failureNode:null)||steps.find(step=>step.state==='failed'&&step.kind!=='finish')||failureNode;
    const terminal=run.workflow?.nodes.find(node=>node.data.kind==='finish'&&run.steps?.[node.id]?.state==='succeeded');
    const cancelled=steps.find(step=>step.state==='cancelled');
    const outcome=run.state==='failed'?failed?.kind==='finish'?failed.label:failed?`${failed.label}에서 실패`:'흐름 실행 실패':run.state==='cancelled'?cancelled?.label||'실행 취소':run.state==='running'?run.activeNodeIds?.length>1?`${run.activeNodeIds.length}개 단계 동시 실행`:run.steps?.[run.activeNodeId]?.label||'실행 중':terminal?.data.label||'흐름 완료';
    const winner=steps.flatMap(step=>step.branchResults||[]).find(branch=>branch.isWinner);
    const parts=path(run),route=parts.map(part=>part.kind==='parallel'?`병렬 ${part.branches.length}개`:part.decision?`${part.label} (${part.decision})`:part.label);
    return {outcome,focusNodeId:failed?.id||terminal?.id||run.activeNodeId||cancelled?.id||executed.at(-1)?.id,executed:executed.length,skipped:steps.filter(step=>step.state==='skipped').length,cancelled:steps.filter(step=>step.state==='cancelled').length,retries:executed.reduce((total,step)=>total+Math.max(0,(step.attempt||1)-1),0),exitCode:failed?.exitCode??(run.state==='succeeded'?0:null),winner:winner?`${winner.label} 결과 채택`:'',route,parts};
  }

  function logs(run){
    return Object.values(run?.steps||{}).flatMap((step,stepOrder)=>(step.logs||[]).map((entry,index)=>({...entry,stepId:step.id,stepLabel:step.label,stepState:step.state,key:`${step.id}:${index}`,stepOrder,index}))).sort((a,b)=>Date.parse(a.time)-Date.parse(b.time)||a.stepOrder-b.stepOrder||a.index-b.index);
  }

  function link(job,run,view='flow',stepId){
    const query=new URLSearchParams({selected:job.name,view:'workflow',scope:'job',panel:'runs',run:run.id,runView:view});
    if(stepId)query.set('step',stepId);
    return `jobs.html?${query}`;
  }

  function renderPath(run){
    function render(parts){return parts.map(part=>part.kind==='parallel'?`<li class="run-path-parallel"><strong>${escape(part.label)}</strong><div>${part.branches.map(branch=>`<section class="${branch.isWinner?'is-winner':''}"><header><span>${escape(branch.label)}</span><small>${branch.isWinner?'채택':escape(labels[branch.state]||'')}</small></header>${branch.steps.length?`<ol>${render(branch.steps)}</ol>`:'<span class="run-path-muted">실행하지 않음</span>'}</section>`).join('')}</div></li>`:`<li class="run-path-node state-${escape(part.state)} ${part.isFailureHandled?'is-handled':''}"><i aria-hidden="true"></i><span>${escape(part.label)}</span>${part.decision?`<small>${escape(part.decision)}</small>`:''}<span class="sr-only">${escape(labels[part.state])}</span></li>`).join('');}
    const parts=path(run);
    return parts.length?`<ol class="run-path" aria-label="실제로 실행한 경로">${render(parts)}</ol>`:'<p class="run-path-muted">아직 시작한 단계가 없습니다.</p>';
  }

  window.SupervisorRunPresentation={list,summary,path,logs,link,renderPath,date,duration,labels};
})();
