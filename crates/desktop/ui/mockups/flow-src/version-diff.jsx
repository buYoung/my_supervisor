import React,{useEffect,useRef,useState} from 'react';
import {historyTime} from './workflow-history.jsx';

export const changeLabels={added:'추가',removed:'삭제',modified:'수정',moved:'이동',unchanged:'변경 없음'};
const changeSymbols={added:'+',removed:'−',modified:'~',moved:'↕',unchanged:'='};
const display=value=>value===undefined?'없음':value===''?'(빈 값)':typeof value==='object'?JSON.stringify(value):String(value);

export function VersionCompareBar({versions,before,after,onBefore,onAfter,onSwap,comparison}) {
  return <div className="version-compare-bar"><div className="version-selectors"><label><span>기준 버전</span><select aria-label="기준 버전" value={before?.id||''} onChange={event=>onBefore(event.target.value)}>{versions.map(version=><option key={version.id} value={version.id}>v{version.number} · {historyTime(version.createdAt)}</option>)}</select></label><button type="button" className="icon-button" aria-label="비교 방향 바꾸기" onClick={onSwap}>⇄</button><label><span>비교 버전</span><select aria-label="비교 버전" value={after?.id||''} onChange={event=>onAfter(event.target.value)}>{versions.map(version=><option key={version.id} value={version.id}>v{version.number} · {historyTime(version.createdAt)}</option>)}</select></label></div><div className="version-compare-counts">{['added','removed','modified'].map(status=><span className={status} key={status}><b>{changeSymbols[status]}</b>{changeLabels[status]} <strong>{comparison.nodeCounts[status]+comparison.edgeCounts[status]}</strong></span>)}<small>단계·연결</small></div></div>;
}
function DiffValue({value,other,side,secret,showSecrets}) {
  if(secret&&!showSecrets)return <code className={value===undefined?'diff-missing':''}>{value===undefined?'없음':'••••••••'}</code>;
  const text=display(value),against=display(other);
  if(value===undefined)return <code className="diff-missing">없음</code>;
  let start=0,end=0;
  while(start<text.length&&start<against.length&&text[start]===against[start])start++;
  while(end<text.length-start&&end<against.length-start&&text[text.length-1-end]===against[against.length-1-end])end++;
  if(other===undefined){start=0;end=0;}
  return <code>{text.slice(0,start)}<mark className={side}>{text.slice(start,text.length-end)||' '}</mark>{end?text.slice(-end):''}</code>;
}
function ConfigDiff({fields,before,after,showSecrets}) {
  if(!fields.length)return <p className="diff-empty">설정값 변경이 없습니다.</p>;
  return <div className="configuration-diff"><div className="config-diff-columns"><span>v{before?.number} · 이전</span><span>v{after?.number} · 이후</span></div>{fields.map(field=><div className={`config-diff-field ${field.status}`} key={field.key}><div className="config-diff-label"><b>{changeSymbols[field.status]}</b>{field.label}{field.secret&&!showSecrets&&<small>값 숨김</small>}</div><div className="config-diff-values"><div><DiffValue value={field.before} other={field.after} side="before" secret={field.secret} showSecrets={showSecrets}/></div><div><DiffValue value={field.after} other={field.before} side="after" secret={field.secret} showSecrets={showSecrets}/></div></div></div>)}</div>;
}
export function VersionDiffDetails({comparison,before,after,nodeId,edgeId,onSelect,onClear,onRestore,isRunning}) {
  const [showSecrets,setShowSecrets]=useState(false);
  const bodyRef=useRef(null);
  useEffect(()=>{bodyRef.current?.scrollTo({top:0});},[nodeId,edgeId,before?.id,after?.id]);
  const node=comparison.nodeChanges.find(change=>change.id===nodeId),edge=comparison.edgeChanges.find(change=>change.id===edgeId),selected=node||edge;
  const priority={modified:0,added:1,removed:2,moved:3};
  const changedNodes=comparison.nodeChanges.filter(change=>change.status!=='unchanged').sort((left,right)=>priority[left.status]-priority[right.status]),changedEdges=comparison.edgeChanges.filter(change=>change.status!=='unchanged');
  const fields=selected?selected.fields:comparison.jobFields;
  return <aside className="job-settings version-diff-details" aria-label="버전 비교 상세"><header><div><span>v{before?.number} → v{after?.number} · {selected?node?'단계 설정 비교':'연결 비교':'읽기 전용'}</span><h2>{selected?selected.label:'변경 내용'}</h2></div>{selected&&<button type="button" className="icon-button" onClick={onClear} aria-label="전체 변경 보기">×</button>}</header><div className="workflow-result-body" ref={bodyRef}>
    {selected?<div className="diff-detail-title"><h3>설정값 diff <span>{fields.length}</span></h3><span className={`diff-selection-status ${selected.status}`}>{changeSymbols[selected.status]} {changeLabels[selected.status]}</span></div>:<><p className="version-diff-guide">단계나 연결을 선택하면 해당 설정의 차이를 볼 수 있습니다.</p>{[['단계',changedNodes,'node'],['연결',changedEdges,'edge']].map(([label,changes,type])=><section className="comparison-change-section" key={type}><h3>{label} <span>{changes.length}</span></h3>{changes.length?changes.map(change=><button type="button" key={change.id} className={`comparison-change-item ${change.status}`} onClick={()=>onSelect(type,change.id)}><b>{changeSymbols[change.status]}</b><span>{change.label}</span><small>{changeLabels[change.status]}</small></button>):<p className="diff-empty">{label} 변경 없음</p>}</section>)}<h3>작업 설정 diff <span>{fields.length}</span></h3></>}
    {fields.some(field=>field.secret)&&<label className="diff-secret-toggle"><input type="checkbox" checked={showSecrets} onChange={event=>setShowSecrets(event.target.checked)}/>환경변수 값 표시</label>}
    <ConfigDiff fields={fields} before={before} after={after} showSecrets={showSecrets}/>
  </div><footer className="version-restore"><button type="button" className="button" disabled={!after||isRunning} onClick={onRestore} title="비교 버전을 초안으로 불러옵니다. 저장하면 새 버전이 됩니다.">v{after?.number}에서 편집 시작</button><small>저장 시 새 버전 생성</small></footer></aside>;
}
