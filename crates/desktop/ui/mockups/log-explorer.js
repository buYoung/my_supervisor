/* Shared process/job log exploration for the standalone desktop mockup. */
(() => {
  const escape=value=>String(value??'').replace(/[&<>"']/g,char=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));
  const number=value=>Number(value).toLocaleString('ko-KR');
  const pageSize=50;
  const groupPreviewSize=3;
  const groupLoadSize=20;
  const presets={attention:['fatal','error','warn','unknown'],all:['fatal','error','warn','info','debug','trace','unknown'],without_info:['fatal','error','warn','debug','trace','unknown']};
  const stageNames={trigger:'시작',command:'명령 실행',condition:'조건 분기',process:'프로세스 제어',health:'준비 상태 확인',wait:'대기',job:'작업 호출',parallel:'병렬 실행',join:'결과 합류',finish:'종료'};

  class SupervisorLogExplorer {
    constructor(options){
      this.root=options.root;this.icon=options.icon;this.save=options.save;this.onSelection=options.onSelection;this.notify=options.notify;
      this.data=window.SupervisorLogData.create(options);this.catalog=this.data.read();this.snapshot=options.snapshot;this.appendCount=0;this.unread=0;this.shouldFollow=true;this.selectedId=null;this.groupIds=new Map();this.openGroups=new Set();this.groupLimits=new Map();
      const saved=options.saved?.revision===2?options.saved:{};
      this.state={revision:2,kind:'process',source:'all',mode:'archive',range:'7',from:this.shiftDate(this.catalog.lastDate,6),to:this.catalog.lastDate,query:'',stream:'all',preset:'attention',levels:[...presets.attention],order:'desc',page:0,wrap:true,grouping:'repeat',runId:'all',stage:'all',lastSources:{process:'all',job:'all'},...saved};
      this.readURL();this.mount();this.render();
    }
    $(selector){return this.root.querySelector(selector);}
    shiftDate(date,days){return new Date(Date.parse(`${date}T00:00:00Z`)-days*86400000).toISOString().slice(0,10);}
    readURL(){
      const params=new URLSearchParams(location.search),source=params.get('source');
      if(params.has('kind'))this.state.kind=params.get('kind')==='job'?'job':'process';
      else if(source&&this.catalog.sources.job.some(item=>item.id===source||item.name===source))this.state.kind='job';
      if(source)this.state.source=this.catalog.sources[this.state.kind].find(item=>item.id===source||item.name===source)?.id||'all';
      if(params.has('run')){this.state.runId=params.get('run');this.state.kind='job';const run=this.catalog.runs.get(this.state.runId);if(run){this.state.source=run.jobId;this.state.from=window.SupervisorLogData.dateOf(Date.parse(run.startedAt));this.state.to=this.state.from;this.state.range='custom';}}
      if(params.has('stage'))this.state.stage=params.get('stage');
      if(params.has('grouping'))this.state.grouping=params.get('grouping')==='none'?'none':'repeat';
      if(params.has('level')&&presets[params.get('level')]){this.state.preset=params.get('level');this.state.levels=[...presets[this.state.preset]];}
      if(params.has('fromDate')){this.state.from=params.get('fromDate');this.state.to=params.get('toDate')||this.state.from;this.state.range='custom';}
      if(!['process','job'].includes(this.state.kind))this.state.kind='process';
      if(!this.catalog.sources[this.state.kind].some(item=>item.id===this.state.source))this.state.source='all';
      if(this.state.runId!=='all'&&!this.catalog.runs.has(this.state.runId))this.state.runId='all';
      if(!Array.isArray(this.state.levels))this.state.levels=[...presets.attention];
      this.state.page=0;
    }
    mount(){
      this.root.innerHTML=`<nav class="log-kind-switch" aria-label="로그 종류"><button type="button" data-log-kind="process"><span>${this.icon('Activity')}프로세스 로그</span><small>실행 중인 프로그램의 출력</small></button><button type="button" data-log-kind="job"><span>${this.icon('ListTree')}작업 로그</span><small>실행 이력과 단계별 출력</small></button></nav>
      <div class="log-workbench"><aside class="log-source-rail"><header><strong id="logs-source-title"></strong><span>현재 기간의 주의 로그</span></header><div id="logs-sources"></div><div class="log-source-note"><strong>수준과 출력은 다릅니다</strong><p>stderr도 INFO일 수 있습니다. 수준을 읽을 수 없는 로그는 ‘미분류’로 표시합니다.</p></div></aside>
      <section class="log-workspace" aria-label="로그 탐색"><header class="log-workspace-heading"><div><span id="logs-kind-label" class="log-origin-badge"></span><h2 id="logs-current-source"></h2><p id="logs-current-description"></p></div><div class="log-display-mode" role="group" aria-label="로그 표시 방식"><button type="button" data-log-mode="archive">${this.icon('Clock3')}기록</button><button type="button" data-log-mode="live">${this.icon('Activity')}실시간</button></div></header>
      <div class="log-scope-bar"><div class="log-date-presets" role="group" aria-label="조회 기간"><button type="button" data-log-range="today">오늘</button><button type="button" data-log-range="7">7일</button><button type="button" data-log-range="30">30일</button><button type="button" data-log-range="all">전체 기간</button></div><form id="logs-dates"><input type="date" aria-label="로그 시작일" required><span>—</span><input type="date" aria-label="로그 종료일" required><button class="button small" type="submit">조회</button></form></div>
      <p id="logs-date-error" role="alert" class="logs-error" hidden></p><details class="log-volume"><summary><span id="logs-retention"></span><span>월별 기록량 ${this.icon('ChevronDown')}</span></summary><div id="logs-months" class="log-months"></div></details>
      <div class="log-job-filters" id="logs-job-filters"><label><span>실행</span><select id="logs-run" aria-label="작업 실행 선택"></select></label><label><span>단계</span><select id="logs-stage" aria-label="작업 단계 선택"></select></label></div>
      <div class="log-filter-main"><label class="log-message-search">${this.icon('Search')}<input type="search" id="logs-search" aria-label="로그 검색" placeholder="메시지·실행 ID·단계 검색" autocomplete="off"></label><select id="logs-preset" aria-label="로그 수준 보기"><option value="attention">주의 로그 · 추천</option><option value="all">전체 로그</option><option value="without_info">INFO만 제외</option><option value="custom">직접 선택</option></select><select id="logs-stream" aria-label="로그 출력 경로" title="stdout/stderr는 로그 수준과 별개입니다"><option value="all">모든 출력</option><option value="stdout">stdout</option><option value="stderr">stderr</option><option value="system">시스템</option></select></div>
      <div class="log-levels" id="logs-levels" role="group" aria-label="표시할 로그 수준"></div><div class="log-filter-explanation" id="logs-filter-explanation"></div>
      <div id="logs-context" class="logs-context" hidden></div><div class="log-results-toolbar"><strong id="logs-results" aria-live="polite"></strong><div class="log-results-actions"><div class="log-grouping-switch" role="group" aria-label="반복 로그 표시 방식"><button type="button" data-log-grouping="repeat">반복 묶기</button><button type="button" data-log-grouping="none">개별 로그</button></div><select id="logs-order" aria-label="로그 정렬"><option value="desc">최신순</option><option value="asc">오래된 순</option></select><label><input type="checkbox" id="logs-wrap">줄바꿈</label><button class="text-button" type="button" data-log-refresh>새로고침</button><button class="text-button" type="button" data-log-export>내보내기</button></div></div>
      <p id="logs-grouping-note" class="logs-grouping-note"></p><div id="logs-live-notice" class="logs-live-notice" hidden></div><div id="logs-lines" class="logs-lines" tabindex="0" aria-label="로그 결과"></div><footer class="logs-pagination"><span id="logs-page-label"></span><div><button class="button small" type="button" data-log-page="previous">이전</button><span id="logs-page-number"></span><button class="button small" type="button" data-log-page="next">다음</button></div><label id="logs-follow-label"><input type="checkbox" id="logs-follow" checked>새 로그 따라가기</label></footer>
      </section></div><p class="logs-footnote">${this.icon('Info')}Asia/Seoul · 한 페이지 50개 항목 · 내보내기는 묶지 않은 원본 기록을 유지합니다. 화면의 데이터는 검토용 샘플입니다.</p>
      <dialog class="log-entry-dialog" aria-labelledby="log-entry-title"><div id="log-entry-content"></div></dialog>`;
      this.root.addEventListener('click',event=>this.onClick(event));
      this.root.addEventListener('toggle',event=>{if(!event.target.matches('.log-repeat-group'))return;const id=event.target.dataset.logGroup;if(event.target.open)this.openGroups.add(id);else this.openGroups.delete(id);},true);
      this.$('#logs-dates').addEventListener('submit',event=>{event.preventDefault();const inputs=this.$('#logs-dates').querySelectorAll('input');if(inputs[0].value>inputs[1].value){this.$('#logs-date-error').hidden=false;this.$('#logs-date-error').textContent='종료일은 시작일과 같거나 이후여야 합니다.';return;}this.state.from=inputs[0].value;this.state.to=inputs[1].value;this.state.range='custom';this.changed();});
      this.$('#logs-search').addEventListener('input',event=>{this.state.query=event.target.value;this.state.page=0;this.filter();this.renderRows();this.persist();});
      const changes={'logs-preset':value=>{this.state.preset=value;if(presets[value])this.state.levels=[...presets[value]];},'logs-stream':value=>this.state.stream=value,'logs-order':value=>this.state.order=value,'logs-run':value=>{this.state.runId=value;this.state.stage='all';const run=this.catalog.runs.get(value);if(run)this.state.source=run.jobId;},'logs-stage':value=>this.state.stage=value};
      for(const [id,change] of Object.entries(changes))this.$(`#${id}`).addEventListener('change',event=>{change(event.target.value);this.changed();});
      this.$('#logs-wrap').addEventListener('change',event=>{this.state.wrap=event.target.checked;this.$('#logs-lines').classList.toggle('no-wrap',!event.target.checked);this.persist();});
      this.$('#logs-follow').addEventListener('change',event=>{this.shouldFollow=event.target.checked;this.unread=0;this.renderRows();});
      this.$('#logs-lines').addEventListener('wheel',event=>{if(this.state.mode==='live'&&event.deltaY<0){this.shouldFollow=false;this.$('#logs-follow').checked=false;this.renderLiveNotice();}},{passive:true});
      this.$('.log-entry-dialog').addEventListener('close',()=>{const previous=this.selectedId;this.selectedId=null;this.renderRows(true);[...this.root.querySelectorAll('[data-log-entry]')].find(button=>button.dataset.logEntry===previous)?.focus({preventScroll:true});});
      this.$('.log-entry-dialog').addEventListener('click',event=>{if(event.target!==event.currentTarget)return;const rect=event.currentTarget.getBoundingClientRect();if(event.clientX<rect.left||event.clientX>rect.right||event.clientY<rect.top||event.clientY>rect.bottom)event.currentTarget.close();});
      const sample=document.createElement('button');sample.type='button';sample.textContent='샘플 로그 추가';sample.addEventListener('click',()=>this.appendSample());document.querySelector('.preview-options').prepend(sample);
    }
    changed(){this.state.page=0;this.context=null;const run=this.catalog.runs.get(this.state.runId);if(run){const date=window.SupervisorLogData.dateOf(Date.parse(run.startedAt));if(this.state.mode==='live'?date!==this.catalog.lastDate:date<this.state.from||date>this.state.to){this.state.runId='all';this.state.stage='all';}}this.$('#logs-date-error').hidden=true;this.render();}
    onClick(event){
      const button=event.target.closest('button');if(!button)return;
      if(button.dataset.logKind){this.state.lastSources[this.state.kind]=this.state.source;this.state.kind=button.dataset.logKind;this.state.source=this.state.lastSources[this.state.kind]||'all';this.state.runId='all';this.state.stage='all';this.changed();}
      if(button.dataset.logSource){this.state.source=button.dataset.logSource;this.state.runId='all';this.state.stage='all';this.changed();}
      if(button.dataset.logMode){this.state.mode=button.dataset.logMode;this.shouldFollow=true;this.unread=0;this.changed();}
      if(button.dataset.logRange){this.state.range=button.dataset.logRange;this.state.to=this.catalog.lastDate;this.state.from=this.state.range==='all'?this.catalog.firstDate:this.shiftDate(this.state.to,this.state.range==='today'?0:Number(this.state.range)-1);this.changed();}
      if(button.dataset.logMonth){const month=button.dataset.logMonth;this.state.range='month';this.state.from=`${month}-01`;const [year,value]=month.split('-').map(Number);this.state.to=[new Date(Date.UTC(year,value,0)).toISOString().slice(0,10),this.catalog.lastDate].sort()[0];this.changed();}
      if(button.dataset.logLevel){const level=button.dataset.logLevel;this.state.levels=this.state.levels.includes(level)?this.state.levels.filter(item=>item!==level):[...this.state.levels,level];this.state.preset='custom';this.changed();}
      if(button.dataset.logGrouping){this.state.grouping=button.dataset.logGrouping;this.state.page=0;this.renderRows();this.persist();}
      if(button.dataset.logGroupMore){const id=button.dataset.logGroupMore;this.groupLimits.set(id,(this.groupLimits.get(id)||groupPreviewSize)+groupLoadSize);this.openGroups.add(id);this.renderRows(true);this.root.querySelector(`[data-log-group-more="${id}"]`)?.focus({preventScroll:true});}
      if(button.dataset.logEntry)this.openDetails(button.dataset.logEntry);
      if(button.dataset.logPage){this.state.page=Math.max(0,Math.min(this.pages-1,this.state.page+(button.dataset.logPage==='next'?1:-1)));this.renderRows();this.persist();}
      if(button.hasAttribute('data-log-refresh')){this.catalog=this.data.read();this.render();this.notify('현재 보관된 로그를 새로 읽었습니다.');}
      if(button.hasAttribute('data-log-export'))this.exportRows();
      if(button.hasAttribute('data-log-reset')){this.state.query='';this.state.stream='all';this.state.preset='all';this.state.levels=[...presets.all];this.state.stage='all';this.changed();}
      if(button.hasAttribute('data-log-expand-range')){this.state.range='all';this.state.from=this.catalog.firstDate;this.state.to=this.catalog.lastDate;this.changed();}
      if(button.hasAttribute('data-log-latest')){this.shouldFollow=true;this.unread=0;this.$('#logs-follow').checked=true;this.renderRows();}
      if(button.hasAttribute('data-entry-close'))this.$('.log-entry-dialog').close();
      if(button.hasAttribute('data-entry-context'))this.showContext();
      if(button.hasAttribute('data-context-back')){this.state={...this.context.previous};this.context=null;this.render();}
      if(button.hasAttribute('data-entry-copy')){const record=this.catalog.records.find(item=>item.id===this.selectedId);navigator.clipboard.writeText(record.raw).then(()=>this.notify('원본 로그를 복사했습니다.')).catch(()=>this.notify('클립보드에 접근할 수 없습니다.'));}
    }
    filter(){
      const s=this.state,q=s.query.toLowerCase().trim();
      this.kindRecords=this.catalog.records.filter(record=>record.kind===s.kind);
      this.sourceRecords=this.kindRecords.filter(record=>s.source==='all'||record.sourceId===s.source);
      this.periodRecords=this.sourceRecords.filter(record=>this.context?Math.abs(record.timestamp-this.context.timestamp)<=120000:s.mode==='live'?record.date===this.catalog.lastDate:record.date>=s.from&&record.date<=s.to);
      this.scopeRecords=this.periodRecords.filter(record=>(s.kind!=='job'||s.runId==='all'||record.runId===s.runId)&&(s.stage==='all'||(s.stage.startsWith('kind:')?record.stageKind===s.stage.slice(5):record.stageId===s.stage))&&(s.stream==='all'||record.stream===s.stream));
      this.levelCounts=Object.fromEntries(window.SupervisorLogData.levels.map(level=>[level,this.scopeRecords.filter(record=>record.level===level).length]));
      this.filtered=this.scopeRecords.filter(record=>s.levels.includes(record.level)&&(!q||`${record.message} ${record.raw} ${record.sourceName} ${record.stageName||''} ${record.runId||''}`.toLowerCase().includes(q))).sort((a,b)=>s.mode==='live'||s.order==='asc'?a.timestamp-b.timestamp:b.timestamp-a.timestamp);
      if(s.grouping==='repeat'){
        this.displayEntries=window.SupervisorLogData.groupRepeated(this.filtered).map(group=>{if(!this.groupIds.has(group.key))this.groupIds.set(group.key,`repeat-${this.groupIds.size+1}`);return {...group,id:this.groupIds.get(group.key)};});
        this.displayEntries.sort((a,b)=>s.mode==='live'?a.last.timestamp-b.last.timestamp:s.order==='asc'?a.first.timestamp-b.first.timestamp:b.last.timestamp-a.last.timestamp);
      }else this.displayEntries=this.filtered.map(record=>({records:[record],first:record,last:record}));
      this.pages=Math.max(1,Math.ceil(this.displayEntries.length/pageSize));this.state.page=Math.min(this.state.page,this.pages-1);
    }
    render(){
      this.catalog=this.data.read();this.filter();this.persist();const s=this.state,$=selector=>this.$(selector);
      for(const button of this.root.querySelectorAll('[data-log-kind]')){button.setAttribute('aria-pressed',String(button.dataset.logKind===s.kind));}
      for(const button of this.root.querySelectorAll('[data-log-mode]'))button.setAttribute('aria-pressed',String(button.dataset.logMode===s.mode));
      for(const button of this.root.querySelectorAll('[data-log-range]'))button.setAttribute('aria-pressed',String(button.dataset.logRange===s.range));
      const sources=this.catalog.sources[s.kind],source=sources.find(item=>item.id===s.source),kindLabel=s.kind==='job'?'작업':'프로세스';
      $('#logs-source-title').textContent=`${kindLabel} ${sources.length}`;
      $('#logs-sources').innerHTML=[{id:'all',name:`전체 ${kindLabel}`},...sources].map(item=>{const count=this.kindRecords.filter(record=>(item.id==='all'||record.sourceId===item.id)&&(s.mode==='live'?record.date===this.catalog.lastDate:record.date>=s.from&&record.date<=s.to)&&window.SupervisorLogData.isAttention(record)).length;return `<button type="button" data-log-source="${escape(item.id)}" aria-pressed="${s.source===item.id}"><span>${escape(item.name)}</span><small title="현재 기간의 경고·오류·미분류">${number(count)}</small></button>`;}).join('');
      $('#logs-kind-label').textContent=`${kindLabel} 로그`;$('#logs-current-source').textContent=source?.name||`전체 ${kindLabel}`;
      $('#logs-current-description').textContent=s.kind==='job'?'실행 ID와 단계를 함께 보관합니다.':'stdout · stderr · 시스템 출력의 수준을 구분합니다.';
      $('.log-scope-bar').hidden=s.mode==='live';$('.log-volume').hidden=s.mode==='live';$('#logs-order').hidden=s.mode==='live';$('#logs-follow-label').hidden=s.mode!=='live';
      const dates=$('#logs-dates').querySelectorAll('input');dates[0].value=s.from;dates[1].value=s.to;for(const input of dates){input.min=this.catalog.firstDate;input.max=this.catalog.lastDate;}
      $('#logs-retention').textContent=`${this.catalog.firstDate.replaceAll('-','.')} — ${this.catalog.lastDate.replaceAll('-','.')} 보관`;
      const months=new Map();for(const record of this.sourceRecords){const value=months.get(record.month)||{all:0,attention:0};value.all++;if(window.SupervisorLogData.isAttention(record))value.attention++;months.set(record.month,value);}
      const maximum=Math.max(1,...[...months.values()].map(item=>item.all));
      $('#logs-months').innerHTML=[...months].sort().map(([month,count])=>`<button type="button" data-log-month="${month}" aria-pressed="${s.from.slice(0,7)===month&&s.to.slice(0,7)===month}" title="전체 ${number(count.all)}줄 · 주의 ${number(count.attention)}줄"><span>${month.slice(2).replace('-','.')}</span><i><b style="width:${Math.max(4,count.all/maximum*100)}%"></b></i><small>${number(count.all)}</small></button>`).join('');
      $('#logs-job-filters').hidden=s.kind!=='job';
      const runs=[...this.catalog.runs.values()].filter(run=>(s.source==='all'||run.jobId===s.source)&&window.SupervisorLogData.dateOf(Date.parse(run.startedAt))>=s.from&&window.SupervisorLogData.dateOf(Date.parse(run.startedAt))<=s.to).sort((a,b)=>Date.parse(b.startedAt)-Date.parse(a.startedAt));
      const chosenRun=this.catalog.runs.get(s.runId),runOptions=runs.slice(0,40);if(chosenRun&&!runOptions.some(run=>run.id===chosenRun.id))runOptions.unshift(chosenRun);
      $('#logs-run').innerHTML='<option value="all">모든 실행</option>'+runOptions.map(run=>`<option value="${escape(run.id)}">${escape(run.jobName)} · ${window.SupervisorLogData.dateOf(Date.parse(run.startedAt)).slice(5,10)} #${run.sequence} · ${run.state==='failed'?'실패':run.state==='running'?'실행 중':'완료'}${run.isArchiveSample?' · 보관 샘플':''}</option>`).join('');$('#logs-run').value=s.runId;
      const stages=chosenRun?chosenRun.workflow.nodes.map(node=>({id:node.id,name:node.data.label})):Object.entries(stageNames).map(([kind,name])=>({id:`kind:${kind}`,name:`${name} 단계`}));
      $('#logs-stage').innerHTML='<option value="all">모든 단계</option>'+stages.map(stage=>`<option value="${escape(stage.id)}">${escape(stage.name)}</option>`).join('');$('#logs-stage').value=s.stage;
      $('#logs-search').value=s.query;$('#logs-preset').value=s.preset;$('#logs-stream').value=s.stream;$('#logs-order').value=s.order;$('#logs-wrap').checked=s.wrap;
      $('#logs-levels').innerHTML=window.SupervisorLogData.levels.map(level=>`<button type="button" class="level-${level}" data-log-level="${level}" aria-pressed="${s.levels.includes(level)}"><span>${window.SupervisorLogData.labels[level]}</span><small>${number(this.levelCounts[level])}</small></button>`).join('');
      $('#logs-filter-explanation').innerHTML=s.preset==='attention'?`<strong>추천 기본값</strong> WARN·ERROR·FATAL과 미분류를 표시합니다. INFO·DEBUG·TRACE는 숨기며 삭제하지 않습니다.`:s.preset==='without_info'?'INFO만 숨깁니다. DEBUG·TRACE는 양이 많을 수 있습니다.':s.preset==='all'?'모든 수준의 로그를 표시합니다.':'선택한 수준만 표시합니다. 수준을 하나도 선택하지 않으면 결과가 비어 있습니다.';
      $('#logs-context').hidden=!this.context;$('#logs-context').innerHTML=this.context?`<span>선택한 로그 전후 2분 · 모든 수준 표시</span><button type="button" data-context-back>이전 필터로 돌아가기</button>`:'';
      this.onSelection({kind:s.kind,source:source?.id,name:source?.name});this.renderRows();
    }
    recordRow(record){
      const kind=record.kind==='job'?'작업':'프로세스';
      return `<button type="button" class="log-result-row level-${record.level} ${record.id===(this.selectedId||this.context?.id)?'is-selected':''}" data-log-entry="${escape(record.id)}" aria-label="${escape(record.localTime)} ${window.SupervisorLogData.labels[record.level]} ${escape(record.sourceName)} 로그 상세"><time datetime="${new Date(record.timestamp).toISOString()}">${record.localTime}</time><span class="log-level">${window.SupervisorLogData.labels[record.level]}</span><span class="log-line-origin"><span class="log-origin-type">${kind}</span><strong>${escape(record.sourceName)}</strong><small>${record.kind==='job'?`${escape(record.runLabel||'실행 ID 없음')} · ${escape(record.stageName||'단계 없음')}${record.isArchiveSample?' · 보관 샘플':''}`:escape(record.stream)}</small></span><code>${escape(record.message)}</code><span class="log-row-open" aria-hidden="true">${this.icon('ChevronRight')}</span></button>`;
    }
    recordRows(records){
      let day='';
      return records.map(record=>{const divider=day===record.date?'':`<div class="logs-day">${record.date.replaceAll('-','.')}<span>Asia/Seoul</span></div>`;day=record.date;return divider+this.recordRow(record);}).join('');
    }
    groupRow(group){
      const record=group.last,count=group.records.length,limit=this.groupLimits.get(group.id)||groupPreviewSize,members=this.state.mode==='live'?[...group.records].reverse():group.records,shown=members.slice(0,limit),remaining=count-shown.length;
      const occurrenceTime=item=>`${item.date.replaceAll('-','.')} ${item.localTime}`;
      return `<details class="log-repeat-group level-${record.level}" data-log-group="${group.id}" ${this.openGroups.has(group.id)?'open':''}><summary aria-controls="${group.id}-members"><span class="log-repeat-heading"><strong class="log-repeat-count">${number(count)}회 반복</strong><span class="log-level">${window.SupervisorLogData.labels[record.level]}</span><span class="log-repeat-origin">${record.kind==='job'?'작업':'프로세스'} · <b>${escape(record.sourceName)}</b>${record.kind==='job'?` · ${escape(record.runLabel||'실행 ID 없음')} · ${escape(record.stageName||'단계 없음')}`:''} · ${escape(record.stream)}</span><span class="log-repeat-chevron">${this.icon('ChevronDown')}</span></span><code class="log-repeat-message">${escape(record.message)}</code><span class="log-repeat-times"><span>최초 <time datetime="${new Date(group.first.timestamp).toISOString()}">${occurrenceTime(group.first)}</time></span><span>최근 <time datetime="${new Date(group.last.timestamp).toISOString()}">${occurrenceTime(group.last)}</time></span></span></summary><div class="log-repeat-members" id="${group.id}-members" role="group" aria-label="반복 로그의 개별 발생 기록"><header><strong>개별 발생 기록</strong><span>${number(shown.length)} / ${number(count)}건 · 클릭하면 원본 보기</span></header>${this.recordRows(shown)}${remaining?`<button type="button" class="log-repeat-more" data-log-group-more="${group.id}">${number(Math.min(groupLoadSize,remaining))}건 더 보기 · ${number(remaining)}건 남음</button>`:''}</div></details>`;
    }
    renderRows(preserveScroll=false){
      const s=this.state,$=selector=>this.$(selector);this.filter();
      const entries=this.displayEntries,start=s.mode==='live'?Math.max(0,entries.length-pageSize):s.page*pageSize,visible=entries.slice(start,start+pageSize),previousTop=$('#logs-lines').scrollTop;
      let day='';
      $('#logs-lines').innerHTML=visible.length?visible.map(group=>{const record=s.mode==='live'||s.order==='desc'?group.last:group.first;const divider=day===record.date?'':`<div class="logs-day">${record.date.replaceAll('-','.')}<span>Asia/Seoul</span></div>`;day=record.date;return divider+(group.records.length>1?this.groupRow(group):this.recordRow(record));}).join(''):`<div class="logs-empty">${this.icon('Search')}<h3>${!s.levels.length?'표시할 수준을 선택하세요':'조건에 맞는 로그가 없습니다'}</h3><p>${this.scopeRecords.length?'현재 수준 필터에서 숨긴 로그가 있습니다.':'선택한 대상과 기간의 로그를 확인하세요.'}</p><div>${this.scopeRecords.length?'<button class="button small" type="button" data-log-reset>전체 수준으로 보기</button>':''}<button class="button small" type="button" data-log-expand-range>전체 기간에서 찾기</button></div></div>`;
      $('#logs-lines').classList.toggle('no-wrap',!s.wrap);
      $('#logs-order').title=s.grouping==='repeat'?'최신순은 최근 발생, 오래된 순은 최초 발생 시각으로 정렬합니다.':'개별 로그의 발생 시각으로 정렬합니다.';
      const hidden=this.scopeRecords.length-this.scopeRecords.filter(record=>s.levels.includes(record.level)).length;
      $('#logs-results').textContent=s.grouping==='repeat'?`${number(entries.length)}개 항목 · 원본 ${number(this.filtered.length)}줄 · ${number(hidden)}줄 수준 필터로 숨김`:`${number(this.filtered.length)}줄 표시 · ${number(hidden)}줄 수준 필터로 숨김`;
      for(const button of this.root.querySelectorAll('[data-log-grouping]'))button.setAttribute('aria-pressed',String(button.dataset.logGrouping===s.grouping));
      $('#logs-grouping-note').textContent=s.grouping==='repeat'?'현재 필터 안에서 같은 대상·실행·단계·수준·출력의 동일한 메시지를 묶습니다. 펼치면 개별 원본을 볼 수 있습니다.':'모든 발생 기록을 묶지 않고 시간순으로 표시합니다.';
      const last=Math.min(start+pageSize,entries.length),unit=s.grouping==='repeat'?'개 항목':'줄';$('#logs-page-label').textContent=entries.length?`${number(start+1)}–${number(last)} / ${number(entries.length)}${unit}`:`0${unit}`;$('#logs-page-number').textContent=s.mode==='live'?`최근 50${unit}`:`${s.page+1} / ${this.pages}`;
      for(const button of this.root.querySelectorAll('[data-log-page]')){button.hidden=s.mode==='live';button.disabled=button.dataset.logPage==='next'?s.page>=this.pages-1:s.page===0;}
      $('[data-log-export]').disabled=!this.filtered.length;$('[data-log-export]').title=`묶지 않은 원본 ${number(this.filtered.length)}줄을 JSONL로 내보내기`;
      this.renderLiveNotice();
      if(s.mode==='live')requestAnimationFrame(()=>{$('#logs-lines').scrollTop=this.shouldFollow?$('#logs-lines').scrollHeight:previousTop;});else $('#logs-lines').scrollTop=preserveScroll?previousTop:0;
    }

    renderLiveNotice(){const notice=this.$('#logs-live-notice');notice.hidden=this.state.mode!=='live';notice.innerHTML=`<span>${this.shouldFollow?'선택한 필터로 최신 출력을 따라갑니다.':'읽던 위치를 유지합니다. 로그 수집은 계속됩니다.'}</span>${!this.shouldFollow?`<button type="button" data-log-latest>새 로그 ${this.unread}줄 · 최신으로</button>`:''}`;}
    persist(){this.save({...this.state});const url=new URL(location.href);url.searchParams.set('kind',this.state.kind);url.searchParams.set('source',this.state.source);url.searchParams.set('level',this.state.preset);url.searchParams.set('grouping',this.state.grouping);for(const [key,value] of [['run',this.state.runId],['stage',this.state.stage]]){if(value==='all')url.searchParams.delete(key);else url.searchParams.set(key,value);}if(this.state.range==='custom'){url.searchParams.set('fromDate',this.state.from);url.searchParams.set('toDate',this.state.to);}else{url.searchParams.delete('fromDate');url.searchParams.delete('toDate');}history.replaceState(history.state,'',url);}
    openDetails(id){
      const record=this.catalog.records.find(item=>item.id===id);if(!record)return;this.selectedId=id;const run=this.catalog.runs.get(record.runId),isJob=record.kind==='job';let pretty=record.raw;try{pretty=JSON.stringify(JSON.parse(record.raw),null,2);}catch{}
      const link=run&&!run.isArchiveSample?window.SupervisorRunPresentation.link({name:record.sourceName},run,'logs',record.stageId):null;
      this.$('#log-entry-content').innerHTML=`<header><div><span class="log-origin-badge">${isJob?'작업 로그':'프로세스 로그'}</span><h2 id="log-entry-title">${escape(record.sourceName)}</h2><p>${record.date} ${record.localTime} · Asia/Seoul</p></div><button class="icon-button" type="button" data-entry-close aria-label="로그 상세 닫기">${this.icon('X')}</button></header><div class="log-entry-body"><dl><div><dt>수준</dt><dd><b class="log-level level-${record.level}">${window.SupervisorLogData.labels[record.level]}</b><small>${escape(record.levelSource)}</small></dd></div><div><dt>출력 경로</dt><dd>${escape(record.stream)}</dd></div>${isJob?`<div><dt>실행</dt><dd>${escape(record.runLabel||'실행 ID 없음')} · ${escape(window.SupervisorRunPresentation.labels[record.runState]||'—')}<small>${escape(record.version||'버전 정보 없음')}${record.isArchiveSample?' · 생성된 보관 샘플':''}</small></dd></div><div><dt>단계</dt><dd>${escape(record.stageName||'정보 없음')}</dd></div>`:''}</dl><section><div class="log-entry-section-heading"><h3>원본 로그</h3><button class="text-button" type="button" data-entry-copy>복사</button></div><pre tabindex="0">${escape(pretty)}</pre></section>${run?`<section><h3>이 실행에서 지나간 흐름</h3>${window.SupervisorRunPresentation.renderPath(run)}</section>`:''}<details class="log-entry-identifiers"><summary>식별 정보</summary><code>${escape(record.id)}</code>${record.runId?`<code>${escape(record.runId)}</code>`:''}</details></div><footer><button class="button" type="button" data-entry-context>앞뒤 로그 함께 보기</button>${link?`<a class="button primary" href="${escape(link)}">실행 흐름·상세 로그</a>`:!isJob?`<a class="button" href="index.html?selected=${encodeURIComponent(record.sourceId)}">프로세스 개요</a>`:''}</footer>`;
      if(!this.$('.log-entry-dialog').open)this.$('.log-entry-dialog').showModal();
    }
    showContext(){const record=this.catalog.records.find(item=>item.id===this.selectedId);if(!record)return;const previous=this.context?.previous||{...this.state,levels:[...this.state.levels]};this.state={...this.state,kind:record.kind,source:record.sourceId,runId:record.kind==='job'?record.runId||'all':'all',stage:'all',stream:'all',query:'',preset:'all',levels:[...presets.all],mode:'archive',page:0,order:'asc',grouping:'none'};this.context={id:record.id,timestamp:record.timestamp,previous};this.$('.log-entry-dialog').close();this.render();}
    exportRows(){const content=this.filtered.map(({id,kind,sourceId,sourceName,timestamp,level,stream,message,raw,runId,stageId,stageName})=>JSON.stringify({id,kind,sourceId,source:sourceName,time:new Date(timestamp).toISOString(),level,stream,message,raw,...(kind==='job'?{runId,stageId,stage:stageName}:{})})).join('\n')+'\n';const url=URL.createObjectURL(new Blob([content],{type:'application/x-ndjson'}));const link=document.createElement('a');link.href=url;link.download=`${this.state.kind}-logs-${this.state.from}-${this.state.to}.jsonl`;link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);this.notify(`현재 필터 결과 ${number(this.filtered.length)}줄을 내보냅니다.`);}
    appendSample(){
      this.appendCount++;const source=this.catalog.sources[this.state.kind].find(item=>item.id===this.state.source)||this.catalog.sources[this.state.kind][0];if(!source)return;
      const level=['info','warn','error','unknown'][(this.appendCount-1)%4],timestamp=this.sourceRecords.reduce((latest,record)=>Math.max(latest,record.timestamp),Date.parse(this.snapshot))+1000;
      const record=window.SupervisorLogData.normalize({id:`live-${timestamp}`,kind:this.state.kind,sourceId:source.id,sourceName:source.name,timestamp,level:level==='unknown'?undefined:level,stream:level==='error'?'stderr':'stdout',message:level==='info'?'정기 상태 확인 완료':level==='warn'?'응답 지연으로 재시도를 예약했습니다.':level==='error'?'요청 실패: 연결 제한 시간을 초과했습니다.':'레벨 필드가 없는 원본 출력'});
      this.data.append(record);this.catalog=this.data.read();this.filter();if(!this.shouldFollow&&this.filtered.some(item=>item.id===record.id))this.unread++;this.render();this.notify(`${window.SupervisorLogData.labels[record.level]} 샘플 로그 추가${this.state.levels.includes(record.level)?'':' · 현재 수준 필터에서 숨김'}`);
    }
  }
  window.SupervisorLogExplorer=SupervisorLogExplorer;
})();
