/* Local .env parsing and a true base / local / incoming merge for the HTML prototype. */
(() => {
  const escape = value => String(value ?? '').replace(/[&<>"']/g,char=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'})[char]);
  const valueAt = (source,key) => ({present:Object.hasOwn(source,key),value:Object.hasOwn(source,key)?source[key]:''});
  const equal = (left,right) => left.present===right.present && (!left.present || left.value===right.value);
  const copy = value => ({...value});

  function changedTextRanges(reference,text) {
    if(reference===text)return [];
    const before=[...reference],after=[...text];
    let prefix=0,beforeEnd=before.length,afterEnd=after.length;
    while(prefix<beforeEnd&&prefix<afterEnd&&before[prefix]===after[prefix])prefix++;
    while(beforeEnd>prefix&&afterEnd>prefix&&before[beforeEnd-1]===after[afterEnd-1]){beforeEnd--;afterEnd--;}
    const left=before.slice(prefix,beforeEnd),right=after.slice(prefix,afterEnd),ranges=[];
    if(!left.length||!right.length||(left.length+1)*(right.length+1)>65536)ranges.push({start:prefix,end:afterEnd});
    else {
      const columns=right.length+1,lengths=new Uint32Array((left.length+1)*columns);
      for(let leftIndex=left.length-1;leftIndex>=0;leftIndex--) {
        for(let rightIndex=right.length-1;rightIndex>=0;rightIndex--) {
          const offset=leftIndex*columns+rightIndex;
          lengths[offset]=left[leftIndex]===right[rightIndex]?lengths[offset+columns+1]+1:Math.max(lengths[offset+columns],lengths[offset+1]);
        }
      }
      let leftIndex=0,rightIndex=0;
      while(leftIndex<left.length||rightIndex<right.length) {
        if(leftIndex<left.length&&rightIndex<right.length&&left[leftIndex]===right[rightIndex]){leftIndex++;rightIndex++;}
        else if(rightIndex<right.length&&(leftIndex===left.length||lengths[leftIndex*columns+rightIndex+1]>=lengths[(leftIndex+1)*columns+rightIndex])) {
          ranges.push({start:prefix+rightIndex,end:prefix+rightIndex+1});rightIndex++;
        } else {ranges.push({start:prefix+rightIndex,end:prefix+rightIndex});leftIndex++;}
      }
    }
    const offsets=[0];for(const char of after)offsets.push(offsets[offsets.length-1]+char.length);
    return ranges.map(range=>({start:offsets[range.start],end:offsets[range.end]}));
  }

  function mergeTextRanges(ranges) {
    const merged=[];
    for(const range of ranges.sort((left,right)=>left.start-right.start||left.end-right.end)) {
      const previous=merged[merged.length-1];
      if(previous&&range.start<=previous.end)previous.end=Math.max(previous.end,range.end);
      else merged.push({...range});
    }
    return merged;
  }

  function renderTextChanges(text,ranges) {
    let html='',offset=0;
    for(const range of ranges) {
      html+=escape(text.slice(offset,range.start));
      html+=range.start===range.end?'<span class="merge-word-gap" title="이쪽에 없는 문자 위치"></span>':`<mark class="merge-word-change">${escape(text.slice(range.start,range.end))}</mark>`;
      offset=range.end;
    }
    return html+escape(text.slice(offset));
  }

  function parse(text,includeLocations=false) {
    const environment=Object.create(null);
    const locations=Object.create(null);
    if(text.includes('\0'))return {error:'텍스트 형식의 환경변수 파일을 선택하세요.'};
    const lines=text.replace(/^\uFEFF/,'').replace(/\r\n?/g,'\n').split('\n');
    for(let index=0;index<lines.length;index++) {
      const start=index;
      const line=lines[index].trim();
      if(!line || line.startsWith('#')) continue;
      const match=line.match(/^(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$/);
      if(!match) return {error:`${index+1}행: KEY=VALUE 형식으로 입력하세요.`};
      const key=match[1];let raw=match[2];let value='';
      if(Object.hasOwn(environment,key)) return {error:`${index+1}행: ${key}가 중복되어 있습니다.`};
      if(raw.startsWith('"') || raw.startsWith("'")) {
        const quote=raw[0];let closed=false;
        if(raw.length===1 && index+1<lines.length) raw+='\n'+lines[++index];
        for(let position=1;position<raw.length;position++) {
          const char=raw[position];
          if(quote==='"' && char==='\\' && position+1<raw.length) {
            const next=raw[++position];value+=({n:'\n',r:'\r',t:'\t','"':'"','\\':'\\'})[next] ?? `\\${next}`;continue;
          }
          if(char===quote) {
            const suffix=raw.slice(position+1).trim();
            if(suffix && !suffix.startsWith('#')) return {error:`${index+1}행: 따옴표 뒤에는 주석만 올 수 있습니다.`};
            closed=true;break;
          }
          value+=char;
          if(position===raw.length-1 && index+1<lines.length) raw+='\n'+lines[++index];
        }
        if(!closed) return {error:`${key}: 닫는 따옴표가 없습니다.`};
      } else value=raw.split('#')[0].trimEnd();
      environment[key]=value;
      locations[key]={start,end:index};
    }
    return includeLocations?{environment,locations,lines}:{environment};
  }

  function serialize(environment) {
    return Object.entries(environment).map(([key,value])=>`${key}=${/^[^\s#'"\\]*$/.test(value)?value:JSON.stringify(value)}`).join('\n');
  }

  function merge(base,local,incoming) {
    return [...new Set([...Object.keys(base),...Object.keys(local),...Object.keys(incoming)])].map(key=>{
      const original=valueAt(base,key),left=valueAt(local,key),right=valueAt(incoming,key);
      let result,origin,conflict=false;
      if(equal(left,right)){result=copy(left);origin=equal(original,left)?'unchanged':'same';}
      else if(equal(left,original)){result=copy(right);origin='incoming';}
      else if(equal(right,original)){result=copy(left);origin='local';}
      else {result=copy(left);origin='unresolved';conflict=true;}
      return {key,base:original,local:left,incoming:right,result,origin,conflict,resolved:!conflict,changed:!equal(original,left)||!equal(original,right)};
    });
  }

  class EnvironmentMergeDialog {
    constructor(options) {
      this.icon=options.icon;this.base={...options.base};this.local={...options.local};this.incoming={...options.incoming};
      this.fileName=options.fileName;this.onApply=options.onApply;this.isExample=Boolean(options.isExample);this.navigation='changes';
      this.rows=merge(this.base,this.local,this.incoming);
      this.activeKey=(this.rows.find(row=>!row.resolved)||this.rows.find(row=>this.hasResultDifference(row))||this.rows[0])?.key;
      this.inputError='';this.fileError='';
      this.dialog=document.createElement('dialog');this.dialog.className='env-merge-dialog';this.dialog.setAttribute('aria-labelledby','env-merge-title');
      this.dialog.innerHTML=`<header class="merge-header"><div><div class="merge-title-row">${this.icon('GitMerge')}<h2 id="env-merge-title">환경변수 비교</h2>${this.isExample?'<span>비교 예시</span>':''}</div><p id="merge-editor-hint">중앙의 최종 값을 기준으로 양쪽 파일을 비교합니다. 화살표로 가져오거나 직접 편집하세요.</p></div><button type="button" class="icon-button" data-merge-close aria-label="병합 닫기">${this.icon('X')}</button></header>
        <div class="merge-toolbar"><div class="merge-navigation"><label class="sr-only" for="merge-navigation">이동할 항목</label><select id="merge-navigation"><option value="changes">차이 있는 줄</option><option value="conflicts">미해결 충돌</option></select><button type="button" class="icon-button" data-merge-previous aria-label="이전 변경" title="이전 변경 · Shift+F7">${this.icon('ChevronUp')||this.icon('ArrowUp')}</button><span data-merge-position></span><button type="button" class="icon-button" data-merge-next aria-label="다음 변경" title="다음 변경 · F7">${this.icon('ChevronDown')}</button><span class="merge-count" data-merge-count></span></div><div class="merge-tools"><span class="merge-comparison-note">최종 값 기준</span><details class="merge-bulk-actions" data-merge-bulk><summary>충돌 일괄 해결</summary><div><button type="button" data-merge-all="local">남은 충돌에 현재 값 사용</button><button type="button" data-merge-all="incoming">남은 충돌에 파일 값 사용</button></div></details><button type="button" class="button small" data-merge-open-file>${this.icon('FolderOpen')}다른 파일 열기</button><input type="file" data-merge-file hidden></div></div>
        <div class="merge-error" role="alert" hidden></div>
        <div class="merge-workspace"><div class="merge-pane-track">
          <section class="merge-pane" aria-label="현재 편집본"><header class="merge-pane-heading"><div><strong>현재 편집본</strong><span data-merge-local-label></span></div><span class="merge-readonly">읽기 전용</span></header><div class="merge-code-viewport" data-merge-viewport="local" tabindex="0" aria-label="현재 편집본 전체 내용"><div class="merge-code-surface" data-merge-source="local"></div></div></section>
          ${this.gutterMarkup('local')}
          <section class="merge-pane merge-result-pane" aria-label="최종 적용 파일"><header class="merge-pane-heading"><div><strong>최종 적용 파일</strong><span>여기서 자유롭게 편집하세요</span></div><span class="merge-editable">편집 가능</span></header><div class="merge-code-viewport" data-merge-viewport="result"><div class="merge-code-surface merge-result-surface"><div class="merge-code-decoration" aria-hidden="true"></div><textarea id="merge-result" aria-label="최종 환경변수 전체 편집" aria-describedby="merge-editor-hint" spellcheck="false" autocapitalize="off" autocomplete="off" wrap="off"></textarea></div></div></section>
          ${this.gutterMarkup('incoming')}
          <section class="merge-pane" aria-label="불러온 파일"><header class="merge-pane-heading"><div><strong>불러온 파일</strong><span data-merge-filename></span></div><span class="merge-readonly">읽기 전용</span></header><div class="merge-code-viewport" data-merge-viewport="incoming" tabindex="0" aria-label="불러온 파일 전체 내용"><div class="merge-code-surface" data-merge-source="incoming"></div></div></section>
        </div></div>
        <div class="merge-context-bar"><div class="merge-selection" data-merge-selection></div><span class="merge-cursor" data-merge-cursor></span></div>
        <footer class="merge-footer"><div><strong data-merge-summary role="status"></strong><small>편집란에 반영한 뒤 ‘변경 저장’으로 설정을 확정합니다.</small></div><div class="merge-footer-actions"><button type="button" class="button" data-merge-close>취소</button><button type="button" class="button primary" data-merge-apply>${this.icon('CheckCheck')}최종 값 적용</button></div></footer>`;
      document.body.append(this.dialog);this.editor=this.dialog.querySelector('#merge-result');this.editor.value=this.alignedText();
      const activeIndex=this.rows.findIndex(row=>row.key===this.activeKey);
      const offset=this.editor.value.split('\n').slice(0,activeIndex).reduce((length,line)=>length+line.length+1,0);
      this.editor.setSelectionRange(offset,offset);
      this.bind();this.render();this.dialog.showModal();
      this.dialog.addEventListener('close',()=>this.dialog.remove(),{once:true});
    }

    alignedText() {
      return this.rows.map(row=>row.result.present?serialize({[row.key]:row.result.value}):'').join('\n');
    }

    gutterMarkup(side) {
      return `<div class="merge-action-column"><div class="merge-gutter-heading" aria-hidden="true"></div><div class="merge-code-viewport merge-action-viewport" data-merge-viewport="${side}-actions"><div class="merge-gutter-surface" data-merge-gutter="${side}"></div></div></div>`;
    }

    readResult() {
      const parsed=parse(this.editor.value,true);this.inputError=parsed.error||'';
      if(parsed.error)return parsed;
      for(const row of this.rows) {
        const result=valueAt(parsed.environment,row.key);
        if(!equal(row.result,result)){row.result=result;row.resolved=true;row.origin='manual';}
      }
      for(const key of Object.keys(parsed.environment)) {
        if(this.rows.some(row=>row.key===key))continue;
        this.rows.push({key,base:valueAt(this.base,key),local:valueAt(this.local,key),incoming:valueAt(this.incoming,key),result:valueAt(parsed.environment,key),origin:'manual',conflict:false,resolved:true,changed:true});
      }
      this.rows=this.rows.filter(row=>row.base.present||row.local.present||row.incoming.present||row.result.present);
      return parsed;
    }

    buildAlignment() {
      const parsed=parse(this.editor.value,true),lines=this.editor.value.split('\n');
      const aligned=lines.map(()=>null),used=new Set();
      for(const [key,location] of Object.entries(parsed.locations||{})) {
        const row=this.rows.find(item=>item.key===key);
        if(row){aligned[location.start]={row};used.add(key);}
      }
      if(parsed.error)lines.forEach((line,index)=>{
        const key=line.match(/^\s*(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=/)?.[1];
        const row=this.rows.find(item=>item.key===key);
        if(row&&!used.has(key)){aligned[index]={row};used.add(key);}
      });
      const gaps=lines.map((line,index)=>!line.trim()&&!aligned[index]?index:-1).filter(index=>index>=0);
      for(const row of this.rows) {
        if(used.has(row.key)||!(row.local.present||row.incoming.present||row.base.present))continue;
        const index=gaps.length?gaps.shift():aligned.length;aligned[index]={row};
      }
      return aligned;
    }

    comparisonValues(row,side) {
      return side==='result'?[row.local,row.incoming]:[row.result];
    }

    hasResultDifference(row) {
      return !equal(row.local,row.result)||!equal(row.incoming,row.result);
    }

    lineClass(row,side,hasDifferences) {
      if(!row)return '';
      const isDifferent=hasDifferences??this.comparisonValues(row,side).some(value=>!equal(value,row[side]));
      const classification=isDifferent?(row.conflict&&!row.resolved?'is-conflict':!row.result.present?'is-deleted':'is-changed'):'';
      return `${classification}${row.key===this.activeKey?' is-active':''}`;
    }

    comparisonRanges(row,side,text) {
      if(!row)return [];
      const references=new Set(this.comparisonValues(row,side).map(value=>value.present?serialize({[row.key]:value.value}):''));
      return mergeTextRanges([...references].flatMap(reference=>changedTextRanges(reference,text)));
    }

    renderSource(side) {
      const sourceLines=new Map(Object.keys(this[side]).map((key,index)=>[key,index+1]));
      return this.alignment.map((entry,index)=>{
        const row=entry?.row,value=row?.[side];
        const text=value?.present?serialize({[row.key]:value.value}):'';
        const ranges=this.comparisonRanges(row,side,text),code=renderTextChanges(text,ranges);
        return `<div class="merge-code-line ${this.lineClass(row,side,ranges.length>0)}" data-merge-line="${index}" ${row?`data-merge-key="${escape(row.key)}" title="${escape(row.key)}${value.present?'':' · 이 파일에 없음'}"`:''}><span class="merge-line-number">${value?.present?sourceLines.get(row.key):''}</span><code>${code||'&nbsp;'}</code></div>`;
      }).join('');
    }

    renderGutter(side) {
      return this.alignment.map((entry,index)=>{
        const row=entry?.row,value=row?.[side],isPending=row?.conflict&&!row.resolved;
        const canTake=row&&(isPending||!equal(value,row.result));
        const action=canTake?`<button type="button" class="merge-line-take" data-merge-take="${side}" data-key="${escape(row.key)}" aria-label="${escape(row.key)} ${side==='local'?'현재':'파일'} 값 사용" title="${value.present?'이 줄을 중앙에 사용':'중앙에서 이 키 삭제'}" ${this.inputError?'disabled':''}>${this.icon(side==='local'?'ArrowRight':'ArrowLeft')}</button>`:'';
        return `<div class="merge-gutter-row ${row?.key===this.activeKey?'is-active':''} ${isPending?'is-pending':''}" data-merge-line="${index}" ${row?`data-merge-key="${escape(row.key)}"`:''}>${action}</div>`;
      }).join('');
    }

    renderResultHighlights() {
      const lines=this.editor.value.split('\n'),parsed=parse(this.editor.value,true),highlighted=new Map();
      for(const [key,location] of Object.entries(parsed.locations||{})) {
        const row=this.rows.find(item=>item.key===key);
        const text=lines.slice(location.start,location.end+1).join('\n');
        const ranges=this.comparisonRanges(row,'result',text);
        let offset=0;
        for(let index=location.start;index<=location.end;index++) {
          const line=lines[index],end=offset+line.length;
          const fragments=ranges.filter(range=>range.start===range.end?range.start>=offset&&range.start<=end:range.start<end&&range.end>offset).map(range=>({start:Math.max(0,range.start-offset),end:Math.min(line.length,range.end-offset)}));
          highlighted.set(index,{row,hasDifferences:fragments.length>0,html:renderTextChanges(line,fragments)});offset=end+1;
        }
      }
      return this.alignment.map((entry,index)=>{
        const line=highlighted.get(index),row=line?.row||entry?.row,text=lines[index]||'';
        const code=line?line.html:row&&!row.result.present?'<span class="merge-removed-marker" title="이 키를 삭제합니다">−</span>':renderTextChanges(text,this.comparisonRanges(row,'result',text));
        return `<div class="merge-code-line ${this.lineClass(row,'result',line?.hasDifferences)}" ${row?`data-merge-key="${escape(row.key)}"`:''}><span class="merge-line-number">${index<lines.length?index+1:''}</span><code class="merge-result-highlight">${code||'&nbsp;'}</code></div>`;
      }).join('');
    }

    render() {
      this.alignment=this.buildAlignment();
      const viewports=[...this.dialog.querySelectorAll('[data-merge-viewport]')];
      const positions=viewports.map(view=>({top:view.scrollTop,left:view.scrollLeft}));
      this.dialog.style.setProperty('--merge-document-height',`${this.alignment.length*26+24}px`);
      const columns=this.editor.value.split('\n').reduce((maximum,line)=>Math.max(maximum,[...line].reduce((length,char)=>length+(char.charCodeAt(0)>255?2:char==='\t'?2:1),0)),1);
      this.dialog.style.setProperty('--merge-result-columns',String(columns));
      for(const side of ['local','incoming']){
        this.dialog.querySelector(`[data-merge-source="${side}"]`).innerHTML=this.renderSource(side);
        this.dialog.querySelector(`[data-merge-gutter="${side}"]`).innerHTML=this.renderGutter(side);
      }
      this.dialog.querySelector('.merge-code-decoration').innerHTML=this.renderResultHighlights();
      viewports.forEach((view,index)=>{view.scrollTop=positions[index].top;view.scrollLeft=positions[index].left;});
      this.dialog.querySelector('[data-merge-local-label]').textContent=this.isExample?'PORT=3200으로 편집한 예시':'저장 전 편집 중인 값';
      const fileLabel=this.dialog.querySelector('[data-merge-filename]');fileLabel.textContent=this.fileName;fileLabel.title=this.fileName;
      this.updateSelection();this.updateSummary();
    }

    navigationRows() {
      return this.rows.filter(row=>this.navigation==='conflicts'?!row.resolved:this.hasResultDifference(row));
    }

    updateSummary() {
      const unresolved=this.rows.filter(row=>!row.resolved).length;
      const changed=this.rows.filter(row=>!equal(row.base,row.result)).length;
      const deleted=this.rows.filter(row=>!row.result.present&&row.base.present).length;
      const navigation=this.navigationRows(),position=navigation.findIndex(row=>row.key===this.activeKey);
      this.dialog.querySelector('[data-merge-position]').textContent=`${position<0?0:position+1} / ${navigation.length}`;
      this.dialog.querySelectorAll('[data-merge-previous], [data-merge-next]').forEach(button=>button.disabled=navigation.length===0);
      this.dialog.querySelector('[data-merge-count]').textContent=`차이 ${this.rows.filter(row=>this.hasResultDifference(row)).length} · 미해결 ${unresolved}`;
      this.dialog.querySelector('[data-merge-bulk]').hidden=unresolved===0;
      this.dialog.querySelectorAll('[data-merge-all]').forEach(button=>button.disabled=Boolean(this.inputError));
      const error=this.dialog.querySelector('.merge-error');error.textContent=this.inputError||this.fileError;error.hidden=!error.textContent;
      this.editor.setAttribute('aria-invalid',String(Boolean(this.inputError)));
      this.dialog.querySelector('[data-merge-summary]').textContent=this.inputError?'최종 파일의 입력 형식을 확인하세요':unresolved?`충돌 ${unresolved}개가 남아 있습니다`:`${changed}개 키 변경${deleted?` · ${deleted}개 삭제`:''} · 적용 준비됨`;
      this.dialog.querySelector('[data-merge-apply]').disabled=Boolean(this.inputError)||unresolved>0;
    }

    updateSelection() {
      const row=this.rows.find(item=>item.key===this.activeKey),selection=this.dialog.querySelector('[data-merge-selection]');
      if(!row){selection.innerHTML='<span>중앙 파일에 KEY=VALUE 형식으로 환경변수를 입력하세요.</span>';return;}
      const status=!row.resolved?'충돌 · 선택 필요':!row.result.present?'삭제':row.origin==='local'?'현재 값 사용':row.origin==='incoming'?'파일 값 사용':row.origin==='manual'?'직접 수정':row.changed?'자동 병합':'변경 없음';
      const base=row.base.present?serialize({[row.key]:row.base.value}):'이 키는 기준 설정에 없습니다';
      selection.innerHTML=`<div class="merge-selected-info"><strong>${escape(row.key)}</strong><span class="merge-selected-state ${row.conflict&&!row.resolved?'unresolved':''}">${status}</span><span class="merge-selected-base" title="${escape(base)}">기준 설정 <code>${escape(base)}</code></span></div><div class="merge-selection-actions">${row.conflict&&!row.resolved?`<button type="button" class="merge-confirm" data-merge-confirm="${escape(row.key)}">이 값으로 확정</button>`:''}${row.result.present?`<button type="button" data-merge-delete="${escape(row.key)}" aria-label="${escape(row.key)} 삭제">키 삭제</button>`:''}</div>`;
      selection.querySelectorAll('button').forEach(button=>button.disabled=Boolean(this.inputError));
      const prefix=this.editor.value.slice(0,this.editor.selectionStart),line=prefix.split('\n').length,column=prefix.length-prefix.lastIndexOf('\n');
      this.dialog.querySelector('[data-merge-cursor]').textContent=`Ln ${line}, Col ${column}`;
    }

    setActiveKey(key) {
      if(!key)return;
      this.activeKey=key;
      this.dialog.querySelectorAll('[data-merge-key]').forEach(element=>element.classList.toggle('is-active',element.dataset.mergeKey===key));
      this.updateSelection();this.updateSummary();
    }

    navigate(direction) {
      const rows=this.navigationRows();if(!rows.length)return;
      const current=rows.findIndex(row=>row.key===this.activeKey);
      const index=current<0?(direction>0?0:rows.length-1):(current+direction+rows.length)%rows.length;
      const row=rows[index];this.setActiveKey(row.key);
      const line=this.alignment.findIndex(entry=>entry?.row===row);
      this.dialog.querySelectorAll('[data-merge-viewport]').forEach(view=>{view.scrollTop=Math.max(0,line*26-view.clientHeight/3);});
      const start=this.editor.value.split('\n').slice(0,line).reduce((offset,text)=>offset+text.length+1,0);
      this.editor.focus({preventScroll:true});this.editor.setSelectionRange(start,start);this.updateSelection();
    }

    writeRow(row,value) {
      const parsed=parse(this.editor.value,true);if(parsed.error)return;
      const lines=parsed.lines,location=parsed.locations[row.key];
      const replacement=value.present?serialize({[row.key]:value.value}):'';
      const index=location?.start??this.buildAlignment().findIndex(entry=>entry?.row===row);
      while(lines.length<=index)lines.push('');
      lines.splice(index,location?location.end-location.start+1:1,replacement);
      this.editor.value=lines.join('\n');row.result=copy(value);row.resolved=true;
      const start=lines.slice(0,index).reduce((offset,line)=>offset+line.length+1,0);
      this.editor.setSelectionRange(start,start+replacement.length);
    }

    select(key,source) {
      const row=this.rows.find(item=>item.key===key);if(!row||this.inputError)return;
      this.writeRow(row,row[source]);row.origin=source;this.activeKey=key;this.render();this.editor.focus({preventScroll:true});
    }

    async readFile(file) {
      if(!file)return;
      if(file.size>512*1024){this.fileError='512 KiB 이하의 텍스트 환경변수 파일을 선택하세요.';this.updateSummary();return;}
      const parsed=parse(await file.text());
      if(!this.dialog.open)return;
      if(parsed.error){this.fileError=`${file.name}: ${parsed.error}`;this.updateSummary();return;}
      const current=this.readResult();if(current.error){this.updateSummary();return;}
      this.local=current.environment;this.incoming=parsed.environment;this.fileName=file.name;this.isExample=false;
      this.rows=merge(this.base,this.local,this.incoming);this.editor.value=this.alignedText();this.fileError='';
      this.activeKey=(this.rows.find(row=>!row.resolved)||this.rows.find(row=>this.hasResultDifference(row))||this.rows[0])?.key;this.render();
    }

    apply() {
      const result=this.readResult();this.render();
      if(result.error||this.rows.some(row=>!row.resolved))return;
      this.dialog.close();this.onApply(result.environment,this.fileName);
    }

    bind() {
      this.dialog.addEventListener('click',event=>{
        const button=event.target.closest('button');
        if(!button){const line=event.target.closest('[data-merge-key]');if(line)this.setActiveKey(line.dataset.mergeKey);return;}
        if(button.hasAttribute('data-merge-close'))this.dialog.close();
        if(button.hasAttribute('data-merge-open-file')){const input=this.dialog.querySelector('[data-merge-file]');input.value='';input.click();}
        if(button.dataset.mergeTake)this.select(button.dataset.key,button.dataset.mergeTake);
        if(button.dataset.mergeAll&&!this.inputError){
          for(const row of this.rows.filter(item=>!item.resolved)){this.writeRow(row,row[button.dataset.mergeAll]);row.origin=button.dataset.mergeAll;}
          this.dialog.querySelector('[data-merge-bulk]').open=false;this.render();
        }
        if(button.dataset.mergeConfirm){const row=this.rows.find(item=>item.key===button.dataset.mergeConfirm);row.resolved=true;row.origin='manual';this.render();}
        if(button.dataset.mergeDelete){const row=this.rows.find(item=>item.key===button.dataset.mergeDelete);this.writeRow(row,{present:false,value:''});row.origin='manual';this.render();}
        if(button.hasAttribute('data-merge-previous'))this.navigate(-1);
        if(button.hasAttribute('data-merge-next'))this.navigate(1);
        if(button.hasAttribute('data-merge-apply'))this.apply();
      });
      this.editor.addEventListener('input',()=>{this.fileError='';this.readResult();this.render();this.selectCursorLine();});
      this.editor.addEventListener('click',()=>this.selectCursorLine());
      this.editor.addEventListener('keyup',()=>this.selectCursorLine());
      this.dialog.addEventListener('keydown',event=>{if(event.key==='F7'){event.preventDefault();this.navigate(event.shiftKey?-1:1);}});
      this.dialog.querySelectorAll('[data-merge-viewport]').forEach(view=>{
        view.addEventListener('scroll',()=>{
          if(this.isSyncingScroll)return;this.isSyncingScroll=true;
          this.dialog.querySelectorAll('[data-merge-viewport]').forEach(other=>{if(other!==view)other.scrollTop=view.scrollTop;});
          requestAnimationFrame(()=>{this.isSyncingScroll=false;});
        });
      });
      this.dialog.addEventListener('change',event=>{
        if(event.target.hasAttribute('data-merge-file'))this.readFile(event.target.files[0]).catch(()=>{this.fileError='파일을 읽지 못했습니다. 다른 텍스트 파일로 다시 시도하세요.';this.updateSummary();});
        if(event.target.id==='merge-navigation'){this.navigation=event.target.value;this.navigate(1);this.updateSummary();}
      });
    }

    selectCursorLine() {
      const line=this.editor.value.slice(0,this.editor.selectionStart).split('\n').length-1;
      const key=this.dialog.querySelector(`.merge-code-decoration .merge-code-line:nth-child(${line+1})`)?.dataset.mergeKey;if(key)this.setActiveKey(key);else this.updateSelection();
    }
  }
  window.SupervisorEnvironment={parse,serialize,merge,EnvironmentMergeDialog};
})();
