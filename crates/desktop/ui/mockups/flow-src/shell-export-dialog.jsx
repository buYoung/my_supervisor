import React,{useEffect,useMemo,useRef,useState} from 'react';
import {createShellExport} from './shell-export.js';

export function ShellExportDialog({jobs,rootIds,group,processes,sourceLabel,onClose}){
  const dialog=useRef(null),[shell,setShell]=useState('bash'),[includeEnvironment,setIncludeEnvironment]=useState(false),[feedback,setFeedback]=useState('');
  const exported=useMemo(()=>createShellExport({jobs,rootIds,group,processes,sourceLabel,shell,includeEnvironment}),[jobs,rootIds,group,processes,sourceLabel,shell,includeEnvironment]);
  useEffect(()=>{dialog.current.showModal();const element=dialog.current;return ()=>element.close();},[]);
  useEffect(()=>setFeedback(''),[shell,includeEnvironment]);
  const download=()=>{const url=URL.createObjectURL(new Blob([exported.script],{type:'text/x-shellscript;charset=utf-8'}));const link=document.createElement('a');link.href=url;link.download=exported.filename;link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);setFeedback(`${exported.filename} 다운로드를 요청했습니다.`);};
  const copy=async()=>{try{await navigator.clipboard.writeText(exported.script);setFeedback('전체 스크립트를 복사했습니다.');}catch{setFeedback('클립보드에 접근할 수 없습니다. 파일로 다운로드하세요.');}};
  return <dialog className="shell-export-dialog" ref={dialog} onCancel={onClose} onClick={event=>{if(event.target===event.currentTarget){const rect=event.currentTarget.getBoundingClientRect();if(event.clientX<rect.left||event.clientX>rect.right||event.clientY<rect.top||event.clientY>rect.bottom)onClose();}}} aria-labelledby="shell-export-title">
    <header><div><span>작업 내보내기</span><h2 id="shell-export-title">셸 스크립트로 내보내기</h2><p>{sourceLabel}</p></div><button className="icon-button" type="button" aria-label="스크립트 내보내기 닫기" onClick={onClose}>×</button></header>
    <div className="shell-export-body"><aside>
      <label className="shell-export-label">실행할 셸</label><div className="shell-export-shells" role="group" aria-label="내보낼 셸"><button type="button" aria-pressed={shell==='bash'} onClick={()=>setShell('bash')}>Bash <small>3.2+</small></button><button type="button" aria-pressed={shell==='zsh'} onClick={()=>setShell('zsh')}>Zsh <small>5+</small></button></div>
      <dl className="shell-export-facts"><div><dt>범위</dt><dd>{group?'그룹 전체':'선택한 작업'}</dd></div><div><dt>포함</dt><dd>{exported.jobCount}개 작업 · {exported.stepCount}개 단계</dd></div><div><dt>파일</dt><dd>{exported.filename}</dd></div></dl>
      <label className="shell-export-environment"><input type="checkbox" checked={includeEnvironment} onChange={event=>setIncludeEnvironment(event.target.checked)}/><span>환경변수 값 포함<small>기본은 값 제외 · 실행 환경에서 주입</small></span></label>
      <div className="shell-export-notes"><strong>실행 전에 확인</strong><ul>{exported.warnings.map(warning=><li key={warning}>{warning}</li>)}</ul>{exported.requirements.length>0&&<p>추가 필요: <code>{exported.requirements.join(', ')}</code></p>}<p>취소된 분기도 완료한 동작은 되돌리지 않습니다.</p></div>
    </aside><section className="shell-export-preview" aria-label="스크립트 미리보기"><div className="shell-export-file"><code>{exported.filename}</code><span>{exported.script.split('\n').length}줄 · UTF-8</span></div>{exported.errors.length?<div className="shell-export-errors" role="alert"><h3>흐름을 먼저 완성하세요</h3><ul>{exported.errors.map(error=><li key={error}>{error}</li>)}</ul></div>:<pre tabIndex="0" aria-label={`${shell} 스크립트 소스`}><code>{exported.script}</code></pre>}</section></div>
    <footer><div role="status">{feedback||<>외부 동작 없이 확인: <code>{shell} {exported.filename} --dry-run</code></>}</div><div><button className="button" type="button" onClick={onClose}>닫기</button><button className="button" type="button" disabled={!exported.script} onClick={copy}>복사</button><button className="button primary" type="button" disabled={!exported.script} onClick={download}>스크립트 다운로드</button></div></footer>
  </dialog>;
}
