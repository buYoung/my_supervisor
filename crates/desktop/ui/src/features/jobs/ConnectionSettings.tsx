import type {FlowNode,Workflow} from '../../services/workspace-types';
import {outputs} from './workflow-model';

export function ConnectionSettings({node,flow,onConnect}:{node:FlowNode;flow:Workflow;onConnect:(handle:string,target:string)=>void}){
 const ports=outputs(node.data);
 if(!ports.length)return null;
 return <fieldset className="settings-section"><legend>다음 단계 연결</legend><p className="editor-note">출력마다 다음 단계를 선택합니다. 결과 분기는 위에서 처음 일치한 순서를 따릅니다.</p>{ports.map(port=><label className="editor-field" key={port.id}><span>{port.label}</span><select aria-label={port.label+" 연결 대상"} value={flow.edges.find(edge=>edge.source===node.id&&edge.sourceHandle===port.id)?.target??""} onChange={event=>onConnect(port.id,event.target.value)}><option value="">연결 없음</option>{flow.nodes.filter(item=>item.id!==node.id&&item.data.kind!=="trigger").map(item=><option key={item.id} value={item.id}>{item.data.label}</option>)}</select></label>)}</fieldset>;
}
