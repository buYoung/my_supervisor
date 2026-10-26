(() => {
  function connectedGroups(nodes, edges) {
    const neighbors = new Map(nodes.map(node => [node.id, new Set()]));
    for (const edge of edges) {
      if (!neighbors.has(edge.source) || !neighbors.has(edge.target)) continue;
      neighbors.get(edge.source).add(edge.target);
      neighbors.get(edge.target).add(edge.source);
    }
    const seen = new Set(), groups = [];
    for (const node of nodes) {
      if (seen.has(node.id) || !neighbors.get(node.id).size) continue;
      const ids = [], pending = [node.id];
      while (pending.length) {
        const id = pending.pop();
        if (seen.has(id)) continue;
        seen.add(id); ids.push(id); pending.push(...neighbors.get(id));
      }
      groups.push({id:`connected:${ids.slice().sort().join('|')}`, nodeIds:ids});
    }
    return groups;
  }
  function jobGroups(jobs) {
    const ids = new Map(jobs.map(job => [job.name, job.id || job.flowId || job.name]));
    const nodes = jobs.map(job => ({...job, id:ids.get(job.name)}));
    const upstream = job => job.upstream || (job.triggerType === 'depends_on' ? String(job.triggerValue || job.expr || '').split(',').map(name => ids.get(name.trim())).filter(Boolean) : []);
    const edges = nodes.flatMap(job => upstream(job).map(source => ({source, target:job.id})));
    const explicit = new Map();
    for(const job of nodes){if(!job.workflowGroupId)continue;const first=explicit.get(job.workflowGroupId);if(first)edges.push({source:first,target:job.id});else explicit.set(job.workflowGroupId,job.id);}
    return connectedGroups(nodes, edges).map(group => {
      const members = nodes.filter(node => group.nodeIds.includes(node.id));
      const root = members.find(node => !upstream(node).length) || members[0];
      return {...group, name:members.find(node => node.connectionGroupName?.trim())?.connectionGroupName || `${root.name} 그룹`};
    });
  }
  function groupFrame(group, positions, width, height) {
    const points = group.nodeIds.map(id => positions[id] || {x:0,y:0});
    const left = Math.min(...points.map(point => point.x)), top = Math.min(...points.map(point => point.y));
    return {position:{x:left-24,y:top-58}, width:Math.max(...points.map(point => point.x))-left+width+48, height:Math.max(...points.map(point => point.y))-top+height+82};
  }
  window.SupervisorFlowGroups = {connectedGroups, jobGroups, groupFrame};
})();
