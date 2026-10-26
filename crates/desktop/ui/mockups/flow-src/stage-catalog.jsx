import React,{useRef,useState} from 'react';
import {stepKinds} from './workflow-model.js';
const entries=[
  {kind:'command',group:'실행',description:'셸 명령, 스크립트, CLI 실행',keywords:'shell node java python bash 실행'},
  {kind:'process',group:'실행',description:'등록된 프로세스 시작·중지·재시작',keywords:'start stop restart 프로세스'},
  {kind:'health',group:'확인',description:'HTTP·TCP·프로세스가 준비될 때까지 확인',keywords:'health readiness ready http tcp 상태 대기',recommended:true},
  {kind:'condition',group:'확인',description:'종료 코드·파일 조건에 따라 참/거짓 분기',keywords:'if branch 조건 파일'},
  {kind:'wait',group:'흐름 제어',description:'정해진 시간 뒤 다음 단계 진행',keywords:'delay sleep 대기'},
  {kind:'parallel',group:'흐름 제어',description:'동시 실행 · 모두 완료 / 첫 성공 / Race 합류',keywords:'parallel fork join race 병렬 동시 합류 경쟁'},
  {kind:'job',group:'흐름 제어',description:'다른 작업을 호출하고 완료 대기',keywords:'subflow job 재사용'},
  {kind:'finish',group:'흐름 제어',description:'성공 또는 실패로 흐름 종료',keywords:'end finish 종료'}
];
const icon=name=><span className="flow-icon" aria-hidden="true" dangerouslySetInnerHTML={{__html:window.SupervisorJobWorkspace.icon(name)}}/>;

export function StageCatalog({onAdd,onClose,onDragStart,onDragEnd}) {
  const [query,setQuery]=useState('');
  const lastDrag=useRef(0);
  const filtered=entries.filter(entry=>`${stepKinds[entry.kind].label} ${entry.description} ${entry.keywords}`.toLowerCase().includes(query.toLowerCase()));
  return <aside className="workflow-stage-catalog" aria-label="단계 추가 목록"><header><div><strong>단계 추가</strong><small>클릭해 삽입 · 끌어서 원하는 위치에 배치</small></div><button type="button" className="icon-button" aria-label="단계 추가 닫기" onClick={onClose}>{icon('X')}</button></header><input aria-label="단계 검색" placeholder="단계 이름·기능 검색" value={query} onChange={event=>setQuery(event.target.value)} autoFocus/>{['실행','확인','흐름 제어'].map(group=>{const items=filtered.filter(entry=>entry.group===group);return items.length>0&&<section key={group}><h3>{group}</h3>{items.map(entry=><button type="button" key={entry.kind} draggable aria-label={`${stepKinds[entry.kind].label} 추가`} onClick={()=>{if(Date.now()-lastDrag.current>250)onAdd(entry.kind);}} onDragStart={event=>{lastDrag.current=Date.now();event.dataTransfer.setData('application/x-msv-step',entry.kind);event.dataTransfer.effectAllowed='copy';onDragStart(entry.kind);}} onDragEnd={()=>{lastDrag.current=Date.now();onDragEnd();}}>{icon(stepKinds[entry.kind].icon)}<span><strong>{stepKinds[entry.kind].label}{entry.recommended&&<em>운영 확인</em>}</strong><small>{entry.description}</small></span><span className="stage-add-affordance" aria-hidden="true">⠿<b>+</b></span></button>)}</section>;})}{!filtered.length&&<p className="history-empty">일치하는 단계가 없습니다.</p>}<p className="stage-policy-hint">클릭하면 선택한 단계 뒤에 연결합니다. 끌어 놓으면 연결하지 않고 배치합니다.</p><p className="stage-policy-hint">재시도·제한 시간은 실행 단계의 설정에서 지정합니다.</p></aside>;
}
