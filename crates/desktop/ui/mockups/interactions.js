/* A local, sample-only prototype. This file never calls the supervisor API. */
(() => {
  'use strict';

  const seed = JSON.parse(document.getElementById('mock-data').textContent);
  const fixtures = window.supervisorMockFixtures;
  seed.processes = fixtures.processes;
  try {
    if(sessionStorage.getItem('msv-mock-fixture-revision')!==fixtures.revision) {
      for(const key of ['processes','view-processes','log-view','history-view']) sessionStorage.removeItem(`msv-mock-${key}`);
      sessionStorage.setItem('msv-mock-fixture-revision',fixtures.revision);
    }
  } catch { /* The fixture defaults still work when storage is unavailable. */ }
  const page = document.body.dataset.page;
  const params = new URLSearchParams(location.search);
  const $ = (selector, root = document) => root.querySelector(selector);
  const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
  const escape = value => String(value ?? '').replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[char]);
  const icon = name => seed.icons[name] || fixtures.icons?.[name] || '';
  const read = (key, fallback) => {
    try { return JSON.parse(sessionStorage.getItem(`msv-mock-${key}`)) || structuredClone(fallback); }
    catch { return structuredClone(fallback); }
  };
  const write = (key, value) => {
    try { sessionStorage.setItem(`msv-mock-${key}`, JSON.stringify(value)); } catch { /* File previews may disable storage. */ }
  };
  let processes = read('processes', seed.processes);
  for(const process of processes) if(process.profileKey && process.profileKey!==process.name) {
    for(const record of fixtures.logs) if(record.source===process.profileKey)record.source=process.name;
  }
  let jobs = read('jobs', seed.jobs);
  let extraRuns = read('runs', {});
  const isInventoryPage = page === 'processes' || page === 'jobs';
  const inventoryState = read(`view-${page}`, { query: '', filter: 'all', selected: null, tab: 'overview' });
  if (params.has('q')) inventoryState.query = params.get('q');
  if (params.has('filter')) inventoryState.filter = params.get('filter');
  if (params.has('selected')) inventoryState.selected = params.get('selected');
  if (params.has('tab')) inventoryState.tab = params.get('tab');
  if(page==='processes' && inventoryState.selected) inventoryState.selected=processes.find(process=>process.name===inventoryState.selected || process.profileKey===inventoryState.selected)?.name || null;
  let selectedProcessFilter = page === 'processes' ? inventoryState.filter : 'all';
  let selectedJobFilter = page === 'jobs' ? inventoryState.filter : 'all';
  const logPreferences = read('log-view', { source: 'api-server', query: '', stream: 'all', shouldAutoScroll: true });
  let logSource = params.get('source') || logPreferences.source;
  logSource=processes.find(process=>process.name===logSource || process.profileKey===logSource)?.name || logSource;
  let logExplorer = null;
  let activePreviewState = 'normal';
  let editorReturnFocus = null;
  let executablePicker = null;
  let environmentEditor = null;
  let menuReturnFocus = null;
  let shouldRetainFiltersOnBack = false;
  const pendingProcesses = new Set();
  const processActionTokens = new Map();
  let toastTimer;
  let dialogAction = null;
  let currentProcess = processes.find(process => process.name === params.get('name') || process.profileKey === params.get('name')) || processes[0];
  let currentJob = jobs.find(job => job.name === params.get('name')) || jobs[0];
  const stateLabel = { running: '실행 중', starting: '시작 중', stopping: '중지 중', stopped: '중지됨', crashed: '비정상 종료', succeeded: '성공', failed: '실패', pending: '대기 중', cancelled: '취소됨' };
  const badge = (state, label = stateLabel[state]) => `<span class="status ${escape(state)}"><span class="dot"></span>${escape(label)}</span>`;
  const iconButton = (name, label, attrs = '') => `<button type="button" class="icon-button" aria-label="${escape(label)}" title="${escape(label)}" ${attrs}>${icon(name)}</button>`;
  const showToast = (message, actionLabel, action) => {
    const toast = $('#toast');
    toast.innerHTML = `<span>${escape(message)}</span>${actionLabel ? `<button type="button" class="toast-action">${escape(actionLabel)}</button>` : ''}`;
    if (actionLabel) $('.toast-action', toast).onclick = () => { action(); toast.hidden = true; };
    toast.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { if (!toast.contains(document.activeElement)) toast.hidden = true; }, 6000);
  };
  function syncWorkflowSettings(job) {
    if(!job.workflow?.nodes)return;
    const start=job.workflow.nodes.find(node=>node.data.kind==='trigger');
    if(start)Object.assign(start.data,{triggerType:job.triggerType,triggerValue:job.triggerValue,timezone:job.timezone||'Asia/Seoul'});
    const command=job.workflow.nodes.find(node=>node.data.kind==='command');
    if(command)Object.assign(command.data,{command:job.command,cwd:job.cwd||'',environment:job.environment||{},executable:job.executable||null,timeoutSeconds:job.timeoutSeconds||''});
  }
  jobs.forEach(syncWorkflowSettings);
  window.SupervisorJobVersions?.recordJobs(jobs);
  if(window.SupervisorJobVersions)write('jobs',jobs);
  const persist = () => { jobs.forEach(syncWorkflowSettings);window.SupervisorJobVersions?.recordJobs(jobs);write('processes', processes); write('jobs', jobs); write('runs', extraRuns); };

  function confirmAction(title, body, label, action, isDanger = false) {
    const dialog = $('#action-dialog');
    $('#dialog-title').textContent = title;
    $('#dialog-body').innerHTML = body;
    $('#dialog-confirm').textContent = label;
    $('#dialog-confirm').className = isDanger ? 'button danger' : 'button primary';
    dialogAction = action;
    $('#context-menu').hidden = true;
    dialog.showModal();
    $('[data-close-dialog]', dialog).focus();
  }

  let themePreference = 'light';
  try { themePreference = localStorage.getItem('msv-mock-theme') || 'light'; } catch { /* Keep local fallback. */ }
  const systemTheme = matchMedia('(prefers-color-scheme: dark)');
  function setTheme(preference, shouldNotify = false) {
    themePreference = preference;
    const theme = preference === 'auto' ? (systemTheme.matches ? 'dark' : 'light') : preference;
    document.documentElement.dataset.theme = theme;
    try { localStorage.setItem('msv-mock-theme', preference); } catch { /* Keep local fallback. */ }
    $$('input[name="theme"]').forEach(input => { input.checked = input.value === preference; });
    const toggle = $('[data-toggle-theme]');
    toggle.innerHTML = icon(theme === 'dark' ? 'Sun' : 'Moon');
    toggle.setAttribute('aria-label', theme === 'dark' ? '라이트 테마로 전환' : '다크 테마로 전환');
    toggle.title = toggle.getAttribute('aria-label');
    if (shouldNotify) showToast('목업의 화면 모양을 변경했습니다.');
  }
  setTheme(themePreference);
  systemTheme.addEventListener('change', () => { if (themePreference === 'auto') setTheme('auto'); });

  function saveInventoryState(shouldPush = false) {
    if (!isInventoryPage) return;
    inventoryState.query = $(`#${page === 'processes' ? 'process' : 'job'}-search`).value;
    inventoryState.filter = page === 'processes' ? selectedProcessFilter : selectedJobFilter;
    write(`view-${page}`, inventoryState);
    const url = new URL(location.href);
    for (const [key, value] of Object.entries({ q: inventoryState.query, filter: inventoryState.filter === 'all' ? '' : inventoryState.filter, selected: inventoryState.selected, tab: inventoryState.selected && inventoryState.tab !== 'overview' ? inventoryState.tab : '' })) {
      if (value) url.searchParams.set(key, value); else url.searchParams.delete(key);
    }
    const state = { ...(history.state || {}), mockPanel: shouldPush || history.state?.mockPanel || false };
    if (shouldPush) history.pushState(state, '', url); else history.replaceState(state, '', url);
  }

  function openInspector(name, tab = 'overview', shouldFocus = false) {
    if (!isInventoryPage || activePreviewState === 'offline' || activePreviewState === 'loading') return;
    const shouldPush = !inventoryState.selected;
    inventoryState.selected = name;
    inventoryState.tab = tab;
    if(page==='processes'){logPreferences.source=name;write('log-view',logPreferences);}
    saveInventoryState(shouldPush);
    renderInspector();
    if (shouldFocus) $('#inspector-title')?.focus({ preventScroll: true });
  }

  function closeInspector() {
    if (!inventoryState.selected) return;
    const name = inventoryState.selected;
    inventoryState.selected = null;
    write(`view-${page}`, inventoryState);
    renderInspector();
    if (history.state?.mockPanel) {
      shouldRetainFiltersOnBack = true;
      history.back();
    } else saveInventoryState();
    $$('[data-inspect]').find(element => element.dataset.inspect === name)?.focus({ preventScroll: true });
  }

  function visibleInventoryRows() {
    return $$(`[data-${page === 'processes' ? 'process' : 'job'}-row]:not([hidden])`);
  }

  function moveInspector(direction) {
    const rows = visibleInventoryRows();
    const index = rows.findIndex(row => row.dataset.name === inventoryState.selected);
    const next = rows[index + direction];
    if (next) openInspector(next.dataset.name, inventoryState.tab);
  }

  function inspectorMetrics(process) {
    const aggregate=(process.instances || 1)>1?' · 합계':'';
    return `<div class="inspector-metrics"><div><span>CPU${aggregate}</span><strong>${escape(process.cpu)}<small>${process.cpu === '—' ? '' : '%'}</small></strong></div><div><span>메모리${aggregate}</span><strong>${escape(process.memory)}<small>${process.memory === '—' ? '' : 'MiB'}</small></strong></div><div><span>재시작</span><strong>${process.restarts}<small>회</small></strong></div></div>`;
  }

  function resourceTrend(values,label,unit) {
    if(!values.length) return `<div class="resource-trend-empty">현재 측정된 ${label} 정보가 없습니다.</div>`;
    const minimum=Math.min(...values)*.8;const maximum=Math.max(...values)*1.12;const span=Math.max(1,maximum-minimum);
    const points=values.map((value,index)=>`${(index/(values.length-1)*280).toFixed(1)},${(47-(value-minimum)/span*40).toFixed(1)}`).join(' ');
    return `<div class="resource-trend"><div><span>${label}</span><small>최고 ${Math.max(...values)}${unit}</small></div><svg viewBox="0 0 280 54" role="img" aria-label="최근 15분 ${label} 추이. 최고 ${Math.max(...values)}${unit}"><path d="M0 48H280" class="trend-baseline"/><polyline points="${points}"/></svg></div>`;
  }

  function processProfile(process) {
    const key=process.profileKey || process.name;
    const base=(!process.isNewMock && Object.hasOwn(fixtures.profiles,key)?fixtures.profiles[key]:null) || {command:process.command || '—',cwd:process.cwd || '기본 작업 디렉터리',environment:{},runtime:'사용자 지정',release:'—',startedAt:'현재 세션',registeredAt:'현재 세션',autostart:false,lifecycle:'런타임과 함께 종료',restart:'자동 재시작',backoff:'1초 → 최대 60초',shutdown:'SIGTERM → 10초 대기',stableAfter:'60초',cpuHistory:[],memoryHistory:[],guards:[],events:[]};
    return {...base,...process.profileOverrides};
  }

  function processOverview(process) {
    const profile=processProfile(process);
    const count=process.instances || 1;
    const isRunning=process.state==='running';
    const records=fixtures.logs.filter(record=>record.source===process.name);
    const failure=process.state==='crashed'?`<div class="inspector-alert">${icon('TriangleAlert')}<div><strong>실행 실패 · ENOENT</strong><p>/usr/local/bin/backup-agent 파일을 찾을 수 없습니다.</p><button type="button" class="text-button" data-inspector-tab="logs">실패 전후 로그 확인 ${icon('ArrowRight')}</button></div></div>`:'';
    const guardRows=profile.guards.map(guard=>{
      const state=isRunning?guard.state:guard.state==='disabled'?'disabled':process.state==='crashed' && guard.label==='생존 확인'?'unhealthy':'unknown';
      const label={healthy:'정상',unhealthy:'실패',unknown:'확인 대기',disabled:'미사용'}[state];
      const summary=guard.label==='메모리 제한' && isRunning?`${process.memory} / ${profile.memoryLimitMiB} MiB`:guard.summary;
      return `<div class="overview-guard"><div><strong>${guard.label}</strong><p>${escape(summary)}</p><small>${escape(guard.detail)}</small></div><span class="status ${state==='healthy'?'running':state==='unhealthy'?'crashed':'neutral'}"><span class="dot"></span>${label}</span></div>`;
    }).join('');
    return `${failure}${inspectorMetrics(process)}
      <section class="overview-section"><div class="inspector-section-title"><h3>자원 추이</h3><span>최근 15분 · 샘플</span></div><div class="resource-trends">${resourceTrend(profile.cpuHistory,'CPU','%')}${resourceTrend(profile.memoryHistory,'메모리',' MiB')}</div>${profile.memoryLimitMiB?`<div class="memory-budget"><span>메모리 한도</span><strong>${process.memory==='—'?'측정 대기':`${process.memory} / ${profile.memoryLimitMiB} MiB`}</strong><div><span style="width:${Math.min(100,Number(process.memory || 0)/profile.memoryLimitMiB*100)||0}%"></span></div></div>`:''}</section>
      <section class="overview-section"><div class="inspector-section-title"><h3>실행 정보</h3><div class="overview-section-actions"><span>${escape(profile.runtime)}</span><button type="button" class="section-edit-button" data-edit-process="${escape(process.name)}" aria-label="${escape(process.name)} 실행 정보 수정">수정</button></div></div>${process.hasPendingExecutionChanges?`<div class="configuration-notice"><p>저장된 설정입니다. 다음 시작 또는 재시작 시 적용됩니다.</p><button type="button" class="text-button" data-process-action="${isRunning?'restart':'start'}" data-name="${escape(process.name)}">${isRunning?'지금 재시작':'지금 시작'} ${icon(isRunning?'RefreshCw':'Play')}</button></div>`:''}<dl class="overview-execution"><div><dt>실행 명령</dt><dd><code>${escape(profile.command)}</code></dd></div><div><dt>작업 디렉터리</dt><dd class="overview-directory"><code title="${escape(profile.cwd)}">${escape(profile.cwd)}</code><button type="button" class="icon-button" data-view-folder="${escape(profile.cwd)}" title="작업 폴더 열기" aria-label="작업 폴더 열기">${icon('FolderOpen')}</button></dd></div><div class="inline-facts"><div><dt>시작 시각</dt><dd>${isRunning?escape(profile.startedAt):'—'}</dd></div><div><dt>가동 시간</dt><dd>${escape(process.uptime)}</dd></div></div><div class="inline-facts"><div><dt>관리 방식</dt><dd>${escape(process.modeLabel)}</dd></div><div><dt>버전</dt><dd class="mono">${escape(profile.release)}</dd></div></div></dl><details class="overview-environment"><summary>환경변수 <span>${Object.keys(profile.environment).length}개 ${icon('ChevronDown')}</span></summary><dl>${Object.entries(profile.environment).map(([key,value])=>`<div><dt>${escape(key)}</dt><dd>${escape(value)}</dd></div>`).join('') || '<div>추가 환경변수 없음</div>'}</dl></details></section>
      <section class="overview-section"><div class="inspector-section-title"><h3>인스턴스</h3><span>${isRunning?count:0} / ${count} 실행</span></div><div class="overview-instance-table"><div class="overview-instance-head"><span>슬롯</span><span>상태</span><span>PID</span><span>메모리</span></div>${Array.from({length:count},(_,index)=>`<div><span class="mono">#${index}</span>${badge(process.state,process.pendingLabel || stateLabel[process.state])}<span class="mono">${isRunning?Number(process.pid)+index:'—'}</span><span class="mono">${isRunning?(Number(process.memory)/count).toFixed(1)+' MiB':'—'}</span></div>`).join('')}</div><form class="inspector-scale" data-inspector-scale><label for="inspector-instance-count">목표 인스턴스</label><input id="inspector-instance-count" name="instances" type="number" min="1" max="20" value="${count}" required><button class="button small" type="submit" disabled>적용</button></form></section>
      <section class="overview-section"><div class="inspector-section-title"><h3>재시작·종료 정책</h3></div><dl class="overview-policy"><div><dt>자동 재시작</dt><dd>${escape(process.mode==='system_registered'?'macOS 서비스에서 관리':profile.restart)}</dd></div><div><dt>재시도 간격</dt><dd>${escape(profile.backoff)}</dd></div><div><dt>정상 종료</dt><dd>${escape(profile.shutdown)}</dd></div><div><dt>자동 시작</dt><dd>${profile.autostart?'사용':'사용 안 함'}</dd></div><div><dt>프로세스 수명</dt><dd>${escape(profile.lifecycle)}</dd></div></dl></section>
      <section class="overview-section"><div class="inspector-section-title"><h3>보호 상태</h3><span>샘플 기준 10:00:00</span></div>${guardRows || '<p class="inspector-hint">설정된 보호 정책이 없습니다.</p>'}</section>
      <section class="overview-section"><div class="inspector-section-title"><h3>최근 이벤트</h3><span>시각 선택 시 해당 날짜 로그</span></div><ol class="overview-events">${profile.events.map(event=>`<li><span class="dot ${event.tone}"></span><div><strong>${escape(event.title)}</strong><p>${escape(event.detail)}</p><a href="logs.html?source=${encodeURIComponent(process.name)}&fromDate=${event.date}&toDate=${event.date}">${event.date.replaceAll('-','.')} ${event.time} ${icon('ArrowUpRight')}</a></div></li>`).join('') || '<li>아직 이벤트가 없습니다.</li>'}</ol></section>
      <section class="overview-log-link"><div><strong>보관된 로그</strong><span>${fixtures.firstDate.replaceAll('-','.')} — ${fixtures.lastDate.replaceAll('-','.')} · ${records.length.toLocaleString('ko-KR')}줄</span></div><a class="text-link" href="logs.html?source=${encodeURIComponent(process.name)}">기록 탐색 ${icon('ArrowRight')}</a></section>`;
  }

  function processInspectorContent(process) {
    if (inventoryState.tab === 'logs') {
      const all=fixtures.logs.filter(record=>record.source===process.name);const lines=all.slice(-8);
      return `<div class="inspector-section-title"><h3>최근 출력</h3><a class="text-link" href="logs.html?source=${encodeURIComponent(process.name)}&from=processes">기록 탐색 ${icon('ArrowUpRight')}</a></div><p class="inspector-hint overview-log-coverage">${fixtures.firstDate.replaceAll('-','.')}부터 ${all.length.toLocaleString('ko-KR')}줄 보관 · 샘플</p><div class="inspector-log" tabindex="0" aria-label="${escape(process.name)} 최근 로그">${lines.map(line=>`<div class="inspector-log-line ${line.stream==='stderr'?'error':''}"><div><time>${line.date.replaceAll('-','.')} ${line.localTime}</time><span>${line.stream}</span></div><code>${escape(line.message)}</code></div>`).join('')}</div><p class="inspector-hint">이전 로그는 기록 탐색에서 월·날짜·메시지로 찾을 수 있습니다.</p>`;
    }
    return processOverview(process);
  }

  function getJobRuns(job) {
    const presentation=window.SupervisorRunPresentation;
    const snapshots=(presentation?.list(job)||[]).filter(run=>run.scope==='flow');
    const recorded=snapshots.map(run=>({id:run.id,date:presentation.date(run.startedAt),state:run.state,duration:run.state==='running'?'실행 중':presentation.duration(run.durationMs),trigger:run.groupName?`${run.groupName}에서 실행`:'작업 흐름 실행',workflowRunId:run.id,snapshot:run,summary:presentation.summary(run)}));
    const ids=new Set(recorded.map(run=>run.id));
    const seedJob=seed.jobs.find(item=>item.name===job.name);
    const historyRows=job.isNewMock?[]:Array.from({length:5},(_,index)=>({id:`sample-${index}`,date:`10.${String(26-index).padStart(2,'0')} ${job.triggerType==='interval'?'14:00':'02:00'}:00`,state:(index===0&&seedJob?.state==='failed')||index===3?'failed':'succeeded',duration:index===3?'7초':`${31-index}초`,trigger:index===2?'수동':'예약'}));
    return recorded.concat((extraRuns[job.name]||[]).filter(run=>!ids.has(run.id)),historyRows).slice(0,5);
  }

  function runLinks(job,run) {
    if(!run.snapshot)return '<p class="run-record-unavailable">이전 요약 기록에는 흐름과 상세 로그가 없습니다.</p>';
    const presentation=window.SupervisorRunPresentation;
    return `<div class="recent-run-actions"><a class="button small" data-job-run="${escape(job.name)}" data-run-id="${escape(run.id)}" data-run-view="flow" href="${escape(presentation.link(job,run.snapshot))}">${icon('ArrowRightLeft')}실행 흐름</a><a class="button small" data-job-run="${escape(job.name)}" data-run-id="${escape(run.id)}" data-run-view="logs" href="${escape(presentation.link(job,run.snapshot,'logs'))}">${icon('FileText')}상세 로그</a></div>`;
  }

  function recentRunCard(job,run,index) {
    const summary=run.summary,presentation=window.SupervisorRunPresentation;
    return `<details class="recent-run-card ${escape(run.state)}" ${index===0?'open':''}><summary><div class="recent-run-heading">${badge(run.state)}<span>${run.snapshot?`#${String(run.snapshot.sequence).padStart(3,'0')} · ${escape(run.snapshot.versionLabel)}`:'이전 요약 기록'}</span>${icon('ChevronDown')}</div><strong>${escape(summary?.outcome||run.trigger)}</strong>${summary?`<p>${summary.executed}개 단계 실행${summary.retries?` · 재시도 ${summary.retries}회`:''}${summary.cancelled?` · 취소 ${summary.cancelled}개`:''}${summary.winner?` · ${escape(summary.winner)}`:''}</p>`:''}<div class="recent-run-time"><time>${escape(run.date)}</time><span>${escape(run.duration)}</span></div></summary><div class="recent-run-body">${run.snapshot?presentation.renderPath(run.snapshot):''}${runLinks(job,run)}<small class="recent-run-id">${escape(run.id)}</small>${run.state==='running'?`<button class="button small" type="button" data-cancel-run="${escape(run.id)}" data-job-name="${escape(job.name)}">실행 취소</button>`:''}</div></details>`;
  }

  function jobInspectorContent(job) {
    if (inventoryState.tab === 'settings') return `<dl class="inspector-facts"><div><dt>일정</dt><dd>${escape(job.trigger)}</dd></div><div><dt>표현식</dt><dd class="mono">${escape(job.expr)}</dd></div><div><dt>시간대</dt><dd>Asia/Seoul</dd></div><div><dt>중복 실행 시</dt><dd>${escape(job.policyLabel)}</dd></div></dl><div class="command-preview"><span class="preview-caption">실행 명령</span><code>${escape(job.command)}</code></div><button class="button" type="button" data-job-workflow="${escape(job.name)}">${icon('ArrowRightLeft')}흐름 편집</button>`;
    const runs=getJobRuns(job);
    return `<button class="button workflow-open-button" type="button" data-job-workflow="${escape(job.name)}">${icon('ArrowRightLeft')}<span><strong>흐름 편집</strong><small>실행 · 조건 · 동작 수정</small></span>${icon('ArrowRight')}</button><div class="inspector-next-run"><span>다음 실행</span><strong>${escape(job.next)}</strong><small>${escape(job.trigger)} · Asia/Seoul</small></div><div class="inspector-section-title"><h3>최근 실행</h3><span>최근 ${runs.length}회</span></div><div class="recent-run-list">${runs.length?runs.map((run,index)=>recentRunCard(job,run,index)).join(''):'<p class="inspector-hint">아직 실행 이력이 없습니다. 첫 실행 결과가 여기에 표시됩니다.</p>'}</div>`;
  }

  function renderInspector() {
    if (!isInventoryPage || !$('#detail-inspector')) return;
    const inspector = $('#detail-inspector');
    const record = (page === 'processes' ? processes : jobs).find(item => item.name === inventoryState.selected);
    $('#normal-view').classList.toggle('has-inspector', Boolean(record));
    $$('[data-process-row], [data-job-row]').forEach(row => { row.dataset.selected = String(row.dataset.name === record?.name); });
    $$('[data-inspect]').forEach(link => { link.setAttribute('aria-expanded', String(link.dataset.inspect === record?.name)); });
    inspector.hidden = !record;
    updateInspectorInteractivity();
    if (!record) return;
    if (page === 'processes') currentProcess = record; else currentJob = record;
    const rows = visibleInventoryRows();
    const index = rows.findIndex(row => row.dataset.name === record.name);
    const isProcess = page === 'processes';
    const isRunning = record.state === 'running';
    const tab = inventoryState.tab;
    const pending = isProcess && pendingProcesses.has(record.name);
    const runningJob=(extraRuns[record.name] || []).find(run=>run.state==='running');
    const primary = isProcess ? `<button class="button ${isRunning ? '' : 'primary'}" type="button" data-process-action="${isRunning?'restart':'start'}" data-name="${escape(record.name)}" ${pending || record.state==='starting'?'disabled':''}>${icon(isRunning?'RefreshCw':'Play')}${pending?escape(record.pendingLabel):record.state==='starting'?'시작 중':isRunning?'재시작':'시작'}</button>${isRunning?`<button class="button" type="button" data-process-action="stop" data-name="${escape(record.name)}" ${pending?'disabled':''}>${icon('Square')}중지</button>`:''}` : `${runningJob?`<button class="button" type="button" data-cancel-run="${escape(runningJob.id)}" data-job-name="${escape(record.name)}">${icon('Square')}실행 취소</button>`:`<button class="button primary" type="button" data-run-job="${escape(record.name)}">${icon('Play')}지금 실행</button>`}`;
    const tabs = isProcess ? [['overview','개요'],['logs','로그']] : [['overview','실행 이력'],['settings','실행 설정']];
    inspector.innerHTML = `<header class="inspector-header"><span>${isProcess?'프로세스':'작업'} 상세</span><div>${iconButton('ArrowLeft', '이전 항목',`data-move-inspector="-1" ${index<=0?'disabled':''}`)}${iconButton('ArrowRight','다음 항목',`data-move-inspector="1" ${index<0 || index>=rows.length-1?'disabled':''}`)}${iconButton('X','상세 닫기','data-close-inspector')}</div></header><div class="inspector-identity"><h2 id="inspector-title" tabindex="-1">${escape(record.name)}</h2>${badge(record.state,record.pendingLabel || stateLabel[record.state])}</div><div class="inspector-actions">${primary}${isProcess?`<button type="button" class="button" data-edit-process="${escape(record.name)}">설정 수정</button>`:''}${iconButton('Ellipsis','추가 동작',`data-${isProcess?'process':'job'}-menu="${escape(record.name)}"`)}</div>${index<0?'<div class="selection-notice">현재 필터에서 숨겨진 항목입니다. <button type="button" data-reveal-selection>필터 해제</button></div>':''}<div class="inspector-tabs" role="tablist" aria-label="상세 보기">${tabs.map(([key,label])=>`<button type="button" role="tab" id="inspector-tab-${key}" data-inspector-tab="${key}" aria-controls="inspector-body" aria-selected="${tab===key}" tabindex="${tab===key?'0':'-1'}">${label}</button>`).join('')}</div><section class="inspector-body" id="inspector-body" role="tabpanel" aria-labelledby="inspector-tab-${tab}">${isProcess?processInspectorContent(record):jobInspectorContent(record)}</section><footer class="inspector-footer"><span><kbd>Esc</kbd> 닫기</span><a class="text-link" href="${isProcess?'process-detail':'job-detail'}.html?name=${encodeURIComponent(record.name)}">별도 페이지 ${icon('ArrowUpRight')}</a></footer>`;
  }

  function refreshViews(shouldPersist = true) {
    if (shouldPersist) persist();
    renderProcesses(); renderJobs(); renderProcessDetail(); renderJobDetail(); updateCounts(); renderInspector();
    if(page==='jobs')window.dispatchEvent(new CustomEvent('supervisor-jobs-changed'));
  }

  function updateInspectorInteractivity() {
    if(!isInventoryPage) return;
    const hasOverlay=!$('#normal-view').hidden && matchMedia('(max-width: 820px)').matches && !$('#detail-inspector').hidden;
    for(const element of $$('.inventory-pane, .page-head, .topbar')) element.inert=hasOverlay;
  }

  function initializeInventory() {
    if (!isInventoryPage) return;
    const normal = $('#normal-view');
    const pane = document.createElement('section');
    pane.className = 'inventory-pane'; pane.setAttribute('aria-label', page === 'processes' ? '프로세스 목록' : '작업 목록');
    while (normal.firstChild) pane.append(normal.firstChild);
    normal.append(pane);
    normal.classList.add('inventory-workbench');
    const inspector = document.createElement('aside');
    inspector.id = 'detail-inspector'; inspector.className = 'detail-inspector'; inspector.hidden = true;
    inspector.setAttribute('aria-label', page === 'processes' ? '프로세스 상세' : '작업 상세');
    normal.append(inspector);
    const search = $(`#${page === 'processes' ? 'process' : 'job'}-search`);
    search.value = inventoryState.query;
    $$(`[data-${page === 'processes' ? 'process' : 'job'}-filter]`).forEach(button => button.setAttribute('aria-pressed', String(button.dataset[page==='processes'?'processFilter':'jobFilter'] === inventoryState.filter)));
    const add = $(`.head-actions a[href="${page === 'processes' ? 'process' : 'job'}-new.html"]`);
    if (add) add.dataset.openEditor = page === 'processes' ? 'process' : 'job';
    if(page==='processes') $('[data-process-filter="attention"]').innerHTML='오류<span data-filter-count="attention">1</span>';
    const alertLink = $('.inventory-pane > .alert a');
    if (alertLink) { alertLink.dataset.inspect = page==='processes'?'backup-agent':'cache-warmup'; alertLink.dataset.inspectTab = page==='processes'?'logs':'overview'; }
    $$('a[href^="job-detail.html"]',pane).forEach(link=>{link.dataset.inspect=new URL(link.href).searchParams.get('name');});
    $$('.nav a').forEach(link=>{link.title=link.getAttribute('aria-label');});
    if(!(page==='processes'?['overview','logs']:['overview','settings']).includes(inventoryState.tab)) inventoryState.tab='overview';
    const hint = document.createElement('p');
    hint.className = 'inventory-hint'; hint.innerHTML = `${page==='jobs'?'클릭하여 상세 보기 · 더블클릭하여 흐름 편집':'항목을 클릭하면 상세가 열립니다.'} <span><kbd>↑</kbd><kbd>↓</kbd> 항목 이동 · <kbd>Esc</kbd> 닫기</span>`;
    pane.append(hint);
  }

  function editorField(label, name, value = '', hint = '', options = '') {
    return `<div class="editor-field"><label for="editor-${name}">${label}</label><input id="editor-${name}" name="${name}" value="${escape(value)}" ${options} aria-describedby="editor-${name}-hint editor-${name}-error" autocomplete="off"><small id="editor-${name}-hint">${hint}</small><p class="field-error" id="editor-${name}-error" hidden></p></div>`;
  }

  function directoryField(value) {
    return `<div class="editor-field"><label for="editor-cwd">작업 디렉터리 <span class="muted">선택</span></label><div class="directory-input"><input id="editor-cwd" name="cwd" class="mono" value="${escape(value || '')}" placeholder="/Users/you/projects/my-app" spellcheck="false" aria-describedby="editor-cwd-hint editor-cwd-error"><button type="button" class="button" id="editor-folder-open">${icon('FolderOpen')}폴더 열기</button></div><small id="editor-cwd-hint">경로를 입력하거나 폴더를 선택하세요. 비워두면 기본 경로를 사용합니다.</small><p id="editor-cwd-error" class="field-error" hidden></p></div>`;
  }

  function parseEnvironmentEntries(value) {
    return window.SupervisorEnvironment.parse(value);
  }

  function renameProcess(process,name) {
    const previousName=process.name;
    if(previousName===name)return;
    if(!process.profileKey && !process.isNewMock && Object.hasOwn(fixtures.profiles,previousName))process.profileKey=previousName;
    process.name=name;
    for(const record of fixtures.logs)if(record.source===previousName)record.source=name;
    if(logPreferences.source===previousName){logPreferences.source=name;write('log-view',logPreferences);}
    const historyView=read('history-view',null);
    if(historyView?.source===previousName){historyView.source=name;write('history-view',historyView);}
    const processView=read('view-processes',null);
    if(processView?.selected===previousName){processView.selected=name;write('view-processes',processView);}
    if(inventoryState.selected===previousName)inventoryState.selected=name;
    if(page==='process-detail'){const url=new URL(location.href);url.searchParams.set('name',name);history.replaceState(history.state,'',url);}
  }

  function openEditor(kind, editingName = '') {
    if(!document.activeElement?.closest('#editor-dialog')) editorReturnFocus = document.activeElement;
    executablePicker?.destroy();executablePicker=null;
    environmentEditor?.destroy();environmentEditor=null;
    let dialog = $('#editor-dialog');
    if (!dialog) {
      dialog = document.createElement('dialog'); dialog.id = 'editor-dialog'; dialog.className = 'editor-dialog'; dialog.setAttribute('aria-labelledby','editor-title'); document.body.append(dialog);
    }
    const editing = (kind === 'job' ? jobs : processes).find(item=>item.name===editingName);
    const draftKey = `draft-${kind}-${editingName || 'new'}`;
    const saved = read(draftKey, null);
    const profile = kind==='process' && editing ? processProfile(editing) : null;
    const values = saved || (profile ? {...editing,command:profile.command,cwd:profile.cwd,environment:window.SupervisorEnvironment.serialize(profile.environment)} : editing) || {};
    const title = editing ? kind==='process'?'프로세스 수정':'작업 수정' : kind === 'process' ? '프로세스 추가' : '작업 추가';
    delete dialog.dataset.completed;
    const environmentField=`<div class="editor-field"><div class="editor-field-heading"><label for="editor-environment">환경변수 <span class="muted">선택</span></label><div><button type="button" class="button small" id="editor-env-open">${icon('FolderOpen')}파일 열기</button><button type="button" class="button small" id="editor-env-example">${icon('FileDiff')}비교 예시</button></div></div><input type="file" id="editor-env-file" hidden><textarea id="editor-environment" class="mono" name="environment" rows="5" spellcheck="false" aria-describedby="editor-environment-hint editor-environment-error" placeholder="NODE_ENV=production">${escape(values.environment || '')}</textarea><small id="editor-environment-hint">.env 파일을 열면 기준·현재·파일 값을 3방향으로 비교합니다.</small><p id="editor-env-merged" class="env-merged-note" hidden role="status"></p><p id="editor-environment-error" class="field-error" hidden></p></div>`;
    const processFields = environmentField+(editing?`<div class="editor-readonly-setting"><span>관리 방식</span><strong>${escape(editing.modeLabel)}</strong><input type="hidden" name="mode" value="${escape(editing.mode)}"><small>관리 방식은 추가 동작의 ‘관리 방식 전환’에서 변경할 수 있습니다.</small></div>`:`<fieldset class="editor-mode"><legend>관리 방식</legend><label><input type="radio" name="mode" value="direct" ${values.mode!=='system_registered'?'checked':''}><span><strong>직접 관리</strong><small>my-supervisor가 직접 실행하고 관리합니다.</small></span></label><label><input type="radio" name="mode" value="system_registered" ${values.mode==='system_registered'?'checked':''}><span><strong>시스템 등록</strong><small>macOS 서비스로 등록하여 관리합니다.</small></span></label></fieldset>`);
    const jobFields = `<div class="editor-field"><label for="editor-triggerType">실행 조건</label><select id="editor-triggerType" name="triggerType"><option value="cron">예약 일정 · cron</option><option value="interval">일정 간격마다</option><option value="one_shot">지정한 시각에 한 번</option><option value="depends_on">다른 작업이 완료된 후</option></select></div>${editorField('cron 표현식','triggerValue',values.triggerValue || '0 2 * * *','','class="mono"')}<div class="schedule-caption" data-editor-schedule></div><div class="editor-field"><label for="editor-policy">이전 실행이 끝나지 않았다면</label><select id="editor-policy" name="policy"><option value="skip">이번 실행 건너뛰기</option><option value="queue">대기열에 추가</option><option value="parallel">동시에 실행</option></select></div>`;
    dialog.innerHTML = `<header class="editor-header"><div><span>${editing?'실행 설정':'새 항목'}</span><h2 id="editor-title">${title}</h2></div>${iconButton('X','입력 창 닫기','data-close-editor')}</header><form id="quick-editor" novalidate><div class="editor-body"><div class="draft-notice" ${saved?'':'hidden'}>이전에 입력한 내용을 복원했습니다.<button type="button" data-clear-draft>새로 입력</button></div>${editing && kind==='process'?'<p class="editor-application-note">이름·명령·작업 디렉터리·환경변수를 수정합니다. 저장한 설정은 다음 시작 또는 재시작 시 적용되며, 저장만으로 실행을 중단하지 않습니다.</p>':''}${editorField('이름','name',values.name || '',kind==='process'?(editing?'이름을 변경해도 기존 로그와 설정은 함께 유지됩니다.':'목록과 로그에서 구분할 이름'):'작업을 식별할 이름',(editing && kind==='job')?'readonly':'placeholder="예: '+(kind==='process'?'api-server':'nightly-backup')+'"')}${editorField('실행 명령','command',values.command || '','터미널에서 실행하는 명령을 입력하세요.','class="mono" placeholder="'+(kind==='process'?'node server.js':'./scripts/backup.sh')+'"')}${kind==='process'?directoryField(values.cwd || '')+processFields:jobFields}</div><footer class="editor-footer"><small>닫아도 이 창에서 입력을 이어갈 수 있습니다.</small><div><button type="button" class="button quiet" data-close-editor>나중에 계속</button><button class="button primary" type="submit">${editing?'변경 저장':'추가'}</button></div></footer></form>`;
    const form = $('#quick-editor');
    if(window.SupervisorExecutablePicker){
      const binding=saved&&Object.hasOwn(saved,'executableBinding')?saved.executableBinding:profile?.executable||editing?.executable;
      executablePicker=new window.SupervisorExecutablePicker({input:form.elements.command,icon,cwd:()=>form.elements.cwd?.value||'',binding,notify:showToast});
      const picker=executablePicker;form.elements.cwd?.addEventListener('input',()=>picker.schedule());
    }
    const triggerValues = saved?.triggerValues || {};
    const saveDraft = () => { write(draftKey, { ...Object.fromEntries(new FormData(form)), executableBinding:executablePicker?.bindingSnapshot(), triggerValues }); };
    form.elements.command.addEventListener('executablechange',event=>{environmentEditor?.setRuntime(event.detail.entry,event.detail.family);saveDraft();});
    if(kind==='process') {
      if(window.SupervisorEnvironmentEditor)environmentEditor=new window.SupervisorEnvironmentEditor({source:form.elements.environment,icon});
      const baseEnvironment=structuredClone(profile?.environment || {});
      const showEnvironmentError=message=>{const error=$('#editor-environment-error',form);error.textContent=message;error.hidden=false;};
      const openEnvironmentMerge=(incoming,fileName,isExample=false)=>{
        const parsed=parseEnvironmentEntries(form.elements.environment.value);
        if(parsed.error){showEnvironmentError(`현재 편집 값: ${parsed.error}`);return;}
        const base=isExample?{...baseEnvironment,HOST:'0.0.0.0',TZ:'Asia/Seoul',LOG_LEVEL:'info',LOG_FORMAT:'json',REQUEST_TIMEOUT_MS:'30000',BODY_LIMIT:'2mb',CORS_ORIGIN:'https://app.example.com',DATABASE_POOL_MIN:'2',DATABASE_POOL_MAX:'20',DATABASE_IDLE_MS:'10000',REDIS_HOST:'127.0.0.1',REDIS_PORT:'6379',REDIS_DB:'0',CACHE_TTL_SECONDS:'300',QUEUE_CONCURRENCY:'4',HEALTH_PATH:'/health',METRICS_ENABLED:'true',GRACEFUL_SHUTDOWN_MS:'10000',LEGACY_METRICS:'false'}:baseEnvironment;
        const local={...(isExample?base:{}),...parsed.environment};
        if(isExample){local.PORT='3200';local.LOG_LEVEL='debug';incoming={...base,...incoming,LOG_LEVEL:'warn',DATABASE_POOL_MAX:'40',CACHE_TTL_SECONDS:'600',QUEUE_CONCURRENCY:'8'};delete incoming.LEGACY_METRICS;}
        new window.SupervisorEnvironment.EnvironmentMergeDialog({base,local,incoming,fileName,isExample,icon,onApply:(environment,loadedFile)=>{
          if(!form.isConnected)return;
          form.elements.environment.value=window.SupervisorEnvironment.serialize(environment);
          form.elements.environment.dispatchEvent(new Event('input',{bubbles:true}));
          const note=$('#editor-env-merged',form);note.textContent=`${loadedFile} · ${Object.keys(environment).length}개 키를 반영했습니다. 변경 저장으로 확정하세요.`;note.hidden=false;
          environmentEditor?.focus();
        }});
      };
      $('#editor-folder-open',form).onclick=()=>new window.SampleFolderDialog({path:form.elements.cwd.value,icon,onChoose:path=>{form.elements.cwd.value=path;form.elements.cwd.dispatchEvent(new Event('input',{bubbles:true}));form.elements.cwd.focus();}});
      $('#editor-env-open',form).onclick=()=>{const input=$('#editor-env-file',form);input.value='';input.click();};
      $('#editor-env-file',form).onchange=async event=>{
        const file=event.target.files[0];if(!file)return;
        if(file.size>512*1024){showEnvironmentError('512 KiB 이하의 텍스트 환경변수 파일을 선택하세요.');return;}
        try {const result=parseEnvironmentEntries(await file.text());if(!dialog.open || !form.isConnected)return;if(result.error){showEnvironmentError(`${file.name}: ${result.error}`);return;}openEnvironmentMerge(result.environment,file.name);}
        catch {showEnvironmentError('파일을 읽지 못했습니다. 텍스트 환경변수 파일을 다시 선택하세요.');}
      };
      $('#editor-env-example',form).onclick=()=>openEnvironmentMerge({NODE_ENV:'production',PORT:'3100',APP_VERSION:'2.4.2',LOG_LEVEL:'debug',FEATURE_FLAGS:'billing,search'},'incoming.env.example',true);
    }
    const configureTrigger = (shouldRestore = true) => {
      if (kind !== 'job') return;
      const type = form.elements.triggerType.value;
      const spec = {cron:['cron 표현식','0 2 * * *','text'],interval:['실행 간격 (초)','900','number'],one_shot:['실행 시각','2026-10-27T02:00','datetime-local'],depends_on:['선행 작업 이름','nightly-backup','text']}[type];
      $('label[for="editor-triggerValue"]',form).textContent = spec[0];
      form.elements.triggerValue.type = spec[2];
      form.elements.triggerValue.min = type==='interval'?'1':'';
      if (shouldRestore) form.elements.triggerValue.value = triggerValues[type] || (values.triggerType===type?values.triggerValue:null) || spec[1];
      const value = form.elements.triggerValue.value;
      $('[data-editor-schedule]',form).textContent = type==='cron' && value==='0 2 * * *'?'매일 02:00 · Asia/Seoul':type==='interval'?`${value || '—'}초마다 실행`:type==='depends_on'?'선행 작업이 완료되면 실행':'Asia/Seoul 기준으로 실행';
    };
    if (kind==='job') {
      form.elements.triggerType.value = values.triggerType || 'cron'; form.elements.policy.value = values.policy || 'skip'; configureTrigger();
      form.elements.triggerType.addEventListener('change',()=>{configureTrigger();saveDraft();});
    }
    form.addEventListener('input',event=>{
      const input = event.target;
      input.removeAttribute('aria-invalid');
      const error = $(`#editor-${input.name}-error`,form); if(error){error.hidden=true;error.textContent='';}
      if(input.name==='environment')$('#editor-env-merged',form).hidden=true;
      if(input.name==='triggerValue'){triggerValues[form.elements.triggerType.value]=input.value;configureTrigger(false);}
      saveDraft();
    });
    form.addEventListener('change',saveDraft);
    form.addEventListener('submit',event=>{
      event.preventDefault();
      const data = Object.fromEntries(new FormData(form));
      const name = data.name.trim(); const command = (executablePicker?.resolvedCommand()||data.command).trim();
      const errors = {};
      const environmentResult=kind==='process'?parseEnvironmentEntries(data.environment):null;
      if(!name) errors.name='이름을 입력하세요.';
      else if((kind==='process'?processes:jobs).some(item=>item!==editing && item.name===name)) errors.name='이미 사용 중인 이름입니다. 다른 이름을 입력하세요.';
      if(!command||executablePicker?.isValid()===false) errors.command='실행할 명령을 입력하세요.';
      if(environmentResult?.error) errors.environment=environmentResult.error;
      if(kind==='job' && !data.triggerValue.trim()) errors.triggerValue='실행 조건을 입력하세요.';
      if(kind==='job' && data.triggerType==='interval' && (!Number.isInteger(Number(data.triggerValue)) || Number(data.triggerValue)<1)) errors.triggerValue='1 이상의 정수(초)를 입력하세요.';
      for(const [key,message] of Object.entries(errors)) {form.elements[key].setAttribute('aria-invalid','true');const error=$(`#editor-${key}-error`,form);error.textContent=message;error.hidden=false;}
      if(Object.keys(errors).length){const first=Object.keys(errors)[0];if(first==='environment')environmentEditor?.focus();else if(first==='command')executablePicker?.focus();else form.elements[first].focus();return;}
      if(kind==='process') {
        const cwd=data.cwd.trim();
        const environment=environmentResult.environment;
        const overrides={...(editing?.profileOverrides || {}),command,cwd,environment};
        const executable=executablePicker?.selection();
        if(executable){overrides.runtime=`${executable.label}${executable.version?' '+executable.version:''}`;overrides.executable=executablePicker.bindingSnapshot();}
        else if(command!==profile?.command){overrides.runtime='사용자 지정';delete overrides.executable;}
        if(editing) {
          const hasChanges=command!==profile.command || cwd!==profile.cwd || JSON.stringify(environment)!==JSON.stringify(profile.environment);
          renameProcess(editing,name);
          Object.assign(editing,{command,cwd,profileOverrides:overrides});
          if(hasChanges && editing.state==='running') editing.hasPendingExecutionChanges=true;
        } else processes.push({name,command,cwd,profileOverrides:overrides,isNewMock:true,mode:data.mode,modeLabel:data.mode==='direct'?'직접 관리':'시스템 등록',state:'stopped',label:'중지됨',pid:'—',cpu:'—',memory:'—',uptime:'—',restarts:0});
      }
      else {
        const trigger = data.triggerType==='cron'?(data.triggerValue==='0 2 * * *'?'매일 02:00':'사용자 지정 일정'):data.triggerType==='interval'?`${data.triggerValue}초마다`:data.triggerType==='depends_on'?'선행 작업 완료 후':'한 번 실행';
        const record={...(editing || {state:'pending',last:'아직 실행되지 않음',success:'—',isNewMock:true}),name,command,executable:executablePicker?.bindingSnapshot(),triggerType:data.triggerType,triggerValue:data.triggerValue,expr:data.triggerValue,policy:data.policy,policyLabel:{skip:'건너뛰기',queue:'대기열에 추가',parallel:'동시 실행'}[data.policy],trigger,next:data.triggerType==='depends_on'?'선행 작업 대기':data.triggerValue==='0 2 * * *'?'내일 02:00':'일정 확인 대기'};
        if(editing) Object.assign(editing,record); else jobs.push(record);
      }
      try{sessionStorage.removeItem(`msv-mock-${draftKey}`);}catch{}
      persist(); dialog.dataset.completed='true';dialog.close();
      if(isInventoryPage){refreshViews();openInspector(name,'overview',true);showToast(editing?'변경 사항을 저장했습니다.':`${name} 추가 완료`,kind==='process'?(editing?(editing.hasPendingExecutionChanges?'지금 재시작':undefined):'지금 시작'):'실행 이력',()=>{if(kind==='process')performProcessAction(name,editing?'restart':'start');else openInspector(name);});}
      else if(editing && kind==='process' && page==='process-detail'){refreshViews();showToast('실행 설정을 저장했습니다.');}
      else location.href=`${kind==='process'?'index':'jobs'}.html?selected=${encodeURIComponent(name)}`;
    });
    $('[data-clear-draft]',dialog).onclick=()=>{try{sessionStorage.removeItem(`msv-mock-${draftKey}`);}catch{}dialog.close();openEditor(kind,editingName);};
    dialog.onclose=()=>{if(dialog.open)return;executablePicker?.destroy();executablePicker=null;environmentEditor?.destroy();environmentEditor=null;const target=dialog.dataset.completed?($('#inspector-title') || $('h1')):(editorReturnFocus?.isConnected && editorReturnFocus.getClientRects().length?editorReturnFocus:$('#inspector-title') || $('[data-open-editor]'));if(target?.tagName==='H1')target.tabIndex=-1;target?.focus({preventScroll:true});};
    dialog.showModal(); if(editing)executablePicker?.focus();else form.elements.name.focus();
  }

  function renderProcesses() {
    if (page !== 'processes') return;
    const tbody = $('.process-table tbody');
    tbody.innerHTML = processes.map(process => {
      const name = escape(process.name);
      const detail = `index.html?selected=${encodeURIComponent(process.name)}`;
      const canStart = ['stopped', 'crashed'].includes(process.state);
      return `<tr data-process-row data-name="${name}" data-state="${escape(process.state)}" aria-busy="${pendingProcesses.has(process.name)}">
        <td><a class="name-link" title="${name}" href="${detail}" data-inspect="${name}" aria-controls="detail-inspector" aria-expanded="${inventoryState.selected===process.name}">${name}</a><span class="cell-sub mono">PID ${escape(process.pid)}</span></td>
        <td>${badge(process.state, process.pendingLabel || stateLabel[process.state])}</td><td class="column-mode"><span class="mode">${icon(process.mode === 'direct' ? 'Monitor' : 'ShieldCheck')}${escape(process.modeLabel)}</span></td>
        <td class="number column-cpu">${escape(process.cpu)}${process.cpu === '—' ? '' : '%'}</td>
        <td class="number">${escape(process.memory)}${process.memory === '—' ? '' : ' <span class="muted">MiB</span>'}</td>
        <td class="number column-uptime">${escape(process.uptime)}</td><td class="number column-restarts">${escape(process.restarts)}</td>
        <td><div class="row-actions">${canStart?`<button type="button" class="row-quick-action" data-process-action="start" data-name="${name}" aria-label="${name} 시작">${icon('Play')}시작</button>`:''}${iconButton('Ellipsis', `${process.name} 더 보기`, `data-process-menu="${name}" aria-haspopup="menu" ${pendingProcesses.has(process.name)?'disabled':''}`)}</div></td></tr>`;
    }).join('');
    const counts = { all: processes.length, running: processes.filter(process => process.state === 'running').length, stopped: processes.filter(process => process.state === 'stopped').length, attention: processes.filter(process => process.state === 'crashed').length };
    $$('[data-filter-count]').forEach(element => { element.textContent = counts[element.dataset.filterCount]; });
    $('[data-page-count]').textContent = String(processes.length).padStart(2, '0');
    $('[data-page-description]').textContent = [[counts.running,'실행 중'],[processes.filter(process=>process.state==='starting').length,'시작 중'],[counts.stopped,'중지됨'],[counts.attention,'비정상 종료']].filter(([count])=>count>0).map(([count,label])=>`${label} ${count}`).join(' · ') || '등록된 프로세스가 없습니다.';
    const banner = $('.inventory-pane > .alert:not(.partial-warning)') || $('#normal-view > .alert');
    if (banner) banner.hidden = !processes.some(p => p.state === 'crashed');
    filterProcesses();
  }

  function filterProcesses() {
    if (page !== 'processes') return;
    const query = $('#process-search').value.trim().toLowerCase();
    let count = 0;
    $$('[data-process-row]').forEach(row => {
      const stateMatches = selectedProcessFilter === 'all' || row.dataset.state === selectedProcessFilter || (selectedProcessFilter === 'attention' && row.dataset.state==='crashed');
      row.hidden = !(stateMatches && row.dataset.name.toLowerCase().includes(query));
      if (!row.hidden) count += 1;
    });
    $('[data-visible-count]').textContent = count;
    $('#search-empty').hidden = count > 0;
    $('.table-wrap').hidden = count === 0;
    saveInventoryState();
    renderInspector();
  }

  function renderProcessDetail() {
    if(page!=='process-detail' || !currentProcess) return;
    const process=currentProcess;
    const isRunning=process.state==='running';
    $('[data-detail-key="name"]').textContent=process.name;
    $('[data-detail-status]').innerHTML=badge(process.state,process.pendingLabel || stateLabel[process.state]);
    $('[data-breadcrumb-title]').textContent=process.name;
    $('[data-page-description]').textContent=`${processProfile(process).runtime} · ${process.modeLabel} · 운영 개요`;
    const buttons=$$('[data-current-process]');
    buttons[0].dataset.processAction=isRunning?'restart':'start';
    buttons[0].innerHTML=icon(isRunning?'RefreshCw':'Play')+(process.pendingLabel || (isRunning?'재시작':'시작'));
    buttons[0].disabled=pendingProcesses.has(process.name);
    buttons[1].disabled=!isRunning || pendingProcesses.has(process.name);
    let logs=$('[data-full-logs]');
    if(!logs){logs=document.createElement('a');logs.className='button';logs.dataset.fullLogs='';logs.innerHTML=icon('Terminal')+'로그 보기';$('.head-actions').prepend(logs);}
    logs.href=`logs.html?source=${encodeURIComponent(process.name)}`;
    $('#normal-view').innerHTML=`<div class="full-process-overview">${processOverview(process)}</div>`;
  }

  function processActionAvailability(process) {
    const profile=processProfile(process),isBusy=pendingProcesses.has(process.name);
    const isActive=['running','starting','stopping'].includes(process.state);
    const busyReason=isBusy?'다른 작업이 진행 중입니다.':'';
    const hasReadiness=profile.guards.some(guard=>guard.label==='준비 상태'&&guard.state!=='disabled');
    const rollingReason=busyReason || (process.state!=='running'?'실행 중일 때 사용할 수 있습니다.':process.mode!=='direct'?'직접 관리에서만 지원합니다.':!profile.rolling?.routability||!hasReadiness?'순차 재시작 정책과 준비 상태 검사가 필요합니다.':'');
    const convertReason=busyReason || (process.mode==='direct'&&((process.instances||1)>1||profile.rolling)?'인스턴스와 실행 보호 정책을 정리한 뒤 전환하세요.':'');
    const removeReason=busyReason || (isActive?'먼저 중지한 뒤 등록을 삭제하세요.':'');
    const canForceStop=isActive&&process.mode==='direct';
    return {isActive,canForceStop,rollingReason,convertReason,removeReason};
  }

  function performProcessAction(name, action) {
    const process=processes.find(item=>item.name===name);
    if(!process || ['offline','loading'].includes(activePreviewState))return;
    if(pendingProcesses.has(name)&&action!=='force-stop'){showToast('현재 작업이 끝난 뒤 다시 시도하세요.');return;}
    const shouldFocusInspector=Boolean(document.activeElement?.closest('#detail-inspector') || (document.activeElement?.closest('#context-menu')&&menuReturnFocus?.closest('#detail-inspector')));
    let token;
    const begin=(label,state=process.state)=>{
      token=Symbol(name);processActionTokens.set(name,token);pendingProcesses.add(name);
      process.state=state;process.pendingLabel=label;refreshViews(false);
      if(shouldFocusInspector)$('#inspector-title')?.focus({preventScroll:true});
    };
    const isCurrent=()=>processActionTokens.get(name)===token;
    const complete=()=>{
      processActionTokens.delete(name);pendingProcesses.delete(name);delete process.pendingLabel;refreshViews();
      if(shouldFocusInspector)$('#inspector-title')?.focus({preventScroll:true});
    };
    const availability=processActionAvailability(process);
    if(action==='remove') {
      if(availability.removeReason){showToast(availability.removeReason);return;}
      confirmAction(`${name} 등록 삭제`, `<p><code>${escape(name)}</code>의 등록 정보와 실행 설정을 삭제합니다.</p><p>작업 디렉터리의 파일과 보관된 로그는 삭제하지 않습니다.</p>`, '등록 삭제',()=>{
        const reason=processActionAvailability(process).removeReason;if(reason){showToast(reason);return;}
        processes=processes.filter(item=>item!==process);persist();
        if(page==='process-detail')location.href='index.html';
        else {if(inventoryState.selected===name){inventoryState.selected=null;saveInventoryState();}refreshViews();showToast(`${name} 등록을 삭제했습니다.`);}
      },true);
    } else if(action==='force-stop') {
      if(!availability.canForceStop)return;
      confirmAction(`${name} 강제 종료`, `<p>정상 종료를 기다리지 않고 모든 인스턴스를 즉시 종료합니다.</p><p>처리 중인 요청과 종료 정리 작업이 중단될 수 있습니다. 프로세스 등록과 보관된 로그는 유지합니다.</p>`, '강제 종료',()=>{
        const current=processActionAvailability(process);
        if(!current.canForceStop){showToast(current.isActive?'시스템 등록 방식에서는 중지를 사용하세요.':`${name}은 이미 중지되었습니다.`);return;}
        processActionTokens.delete(name);pendingProcesses.delete(name);delete process.pendingLabel;
        Object.assign(process,{state:'stopped',pid:'—',cpu:'—',memory:'—',uptime:'—'});refreshViews();
        showToast(`${name} 강제 종료 완료`,'다시 시작',()=>performProcessAction(name,'start'));
      },true);
    } else if(action==='rolling') {
      if(availability.rollingReason){showToast(availability.rollingReason);return;}
      const count=process.instances||1;
      confirmAction(`${name} 순차 재시작`, `<p>${count}개 인스턴스를 하나씩 교체합니다. 새 인스턴스의 준비 상태를 확인한 뒤 기존 인스턴스를 종료합니다.</p><p>준비 상태 확인에 실패하면 교체를 중단하고 기존 인스턴스를 유지합니다.</p>`, '순차 재시작',()=>{
        const reason=processActionAvailability(process).rollingReason;if(reason){showToast(reason);return;}
        let completed=0;begin(`순차 재시작 1 / ${count}`);
        const replaceNext=()=>{
          if(!isCurrent())return;
          completed++;
          if(completed<count){process.pendingLabel=`순차 재시작 ${completed+1} / ${count}`;refreshViews(false);setTimeout(replaceNext,650);return;}
          process.restarts++;process.uptime='방금 시작';delete process.hasPendingExecutionChanges;complete();showToast(`${name} · ${count}개 인스턴스 순차 재시작 완료`);
        };
        setTimeout(replaceNext,650);
      });
    } else if(action==='convert') {
      if(availability.convertReason){showToast(availability.convertReason);return;}
      const isDirect=process.mode==='direct',wasRunning=process.state==='running',previousState=process.state;
      confirmAction('관리 방식 전환', `<p><code>${escape(name)}</code>을 <strong>${isDirect?'시스템 등록':'직접 관리'}</strong>${isDirect?'으로':'로'} 전환합니다.</p><p>${isDirect?'macOS 서비스로 등록합니다.':'macOS 서비스 등록을 해제하고 my-supervisor가 직접 관리합니다.'}${wasRunning?' 전환 중에는 잠시 중지되며, 완료 후 다시 시작합니다.':''}</p>`, '전환',()=>{
        const reason=processActionAvailability(process).convertReason;if(reason){showToast(reason);return;}
        begin('관리 방식 전환 중',wasRunning?'stopping':previousState);
        setTimeout(()=>{
          if(!isCurrent())return;
          process.mode=isDirect?'system_registered':'direct';process.modeLabel=isDirect?'시스템 등록':'직접 관리';process.state=previousState;
          complete();showToast(`${name} · ${process.modeLabel}로 전환했습니다.`);
        },650);
      });
    } else if(['start','stop','restart'].includes(action)) {
      begin(action==='restart'?'재시작 중':stateLabel[action==='stop'?'stopping':'starting'],action==='stop'?'stopping':'starting');
      if(!shouldFocusInspector)$$('[data-inspect]').find(element=>element.dataset.inspect===name)?.focus({preventScroll:true});
      setTimeout(()=>{
        if(!isCurrent())return;
        if(action==='restart')process.restarts++;
        const hasStartFailure=(process.profileKey||name)==='backup-agent'&&processProfile(process).command.startsWith('/usr/local/bin/backup-agent')&&action!=='stop';
        if(action==='stop'||hasStartFailure)Object.assign(process,{state:hasStartFailure?'crashed':'stopped',pid:'—',cpu:'—',memory:'—',uptime:'—'});
        else Object.assign(process,{state:'running',pid:process.pid==='—'?'48501':process.pid,cpu:'0.1',memory:'24.0',uptime:'방금 시작'});
        if(action!=='stop'&&!hasStartFailure)delete process.hasPendingExecutionChanges;
        complete();
        if(hasStartFailure)showToast(`${name} 시작 실패 · 실행 파일을 찾을 수 없습니다.`,'로그 확인',()=>openInspector(name,'logs',true));
        else showToast(`${name} ${action==='stop'?'중지':action==='restart'?'재시작':'시작'} 완료`,action==='stop'?'다시 시작':undefined,()=>performProcessAction(name,'start'));
      },650);
    }
  }

  function latestRunCell(job) {
    const recent=getJobRuns(job)[0];
    if(!recent?.snapshot)return `${badge(job.state)}<span class="cell-sub">${escape(job.last)}</span>`;
    const info=recent.summary;
    return `<a class="latest-run-link" data-job-run="${escape(job.name)}" data-run-id="${escape(recent.id)}" data-run-view="flow" href="${escape(window.SupervisorRunPresentation.link(job,recent.snapshot))}" aria-label="${escape(job.name)} 최근 실행 흐름 보기">${badge(recent.state)}<strong>${escape(info.outcome)}</strong><span>${escape(recent.snapshot.versionLabel)} · ${info.executed}개 단계 · ${escape(recent.duration)}</span><time>${escape(recent.date)}</time></a>`;
  }

  function renderJobs() {
    if (page !== 'jobs') return;
    const groups=window.SupervisorFlowGroups?.jobGroups(jobs)||[];
    const groupByJob=new Map(groups.flatMap(group=>group.nodeIds.map(id=>[id,group])));
    $('.jobs-table tbody').innerHTML = jobs.map(job => `<tr data-job-row title="더블클릭하여 흐름 편집" data-name="${escape(job.name)}" data-state="${escape(job.state)}"><td><a class="name-link" href="jobs.html?selected=${encodeURIComponent(job.name)}" data-inspect="${escape(job.name)}" aria-controls="detail-inspector" aria-expanded="${inventoryState.selected===job.name}">${escape(job.name)}</a><span class="cell-sub">${escape(job.trigger)}</span>${groupByJob.has(job.flowId||job.name)?`<button type="button" class="job-connection-badge" data-job-connections="${escape(job.name)}" aria-label="${escape(job.name)} 연결 그룹 보기" title="연결된 작업 보기">${icon('ArrowRightLeft')}<span>${escape(groupByJob.get(job.flowId||job.name).name)}</span><small>${groupByJob.get(job.flowId||job.name).nodeIds.length}개 작업</small></button>`:''}</td><td class="column-mode"><span class="mono" style="font-size:10px">${escape(job.expr)}</span><span class="cell-sub">${job.triggerType === 'depends_on' ? '선행 작업' : 'Asia/Seoul'}</span></td><td class="column-policy"><span style="font-size:11px">${escape(job.policyLabel)}</span></td><td>${latestRunCell(job)}</td><td class="column-next"><span class="mono" style="font-size:10px">${escape(job.next)}</span></td><td><div class="row-actions"><button type="button" class="button small job-workflow-entry" data-job-workflow="${escape(job.name)}" aria-label="${escape(job.name)} 흐름 편집">${icon('ArrowRightLeft')}흐름 편집</button><button type="button" class="icon-button" data-run-job-single="${escape(job.name)}" aria-label="${escape(job.name)} 이 작업만 실행" title="선행 조건 없이 이 작업만 실행 · 후속 작업 실행 안 함" ${job.state==='running'?'disabled':''}>${icon('Play')}</button>${iconButton('Ellipsis', `${job.name} 더 보기`, `data-job-menu="${escape(job.name)}" aria-haspopup="menu"`)}</div></td></tr>`).join('');
    $('[data-page-count]').textContent = String(jobs.length).padStart(2, '0');
    $('[data-job-filter-count="all"]').textContent = jobs.length;
    $('[data-job-filter-count="failed"]').textContent = jobs.filter(job => job.state === 'failed').length;
    const banner = $('.inventory-pane > .alert:not(.partial-warning)') || $('#normal-view > .alert');
    if (banner) {
      const failed=jobs.filter(job=>job.state==='failed');banner.hidden=!failed.length;
      if(failed.length){const recent=failed.find(job=>extraRuns[job.name]?.[0]?.state==='failed')||failed[0];banner.innerHTML=`${icon('TriangleAlert')}<div><strong>${escape(recent.name)}의 마지막 실행이 실패했습니다.</strong><span class="alert-detail">${failed.length}개 작업 확인 필요 · ${escape(recent.last)}</span></div><button type="button" class="button small" data-inspect="${escape(recent.name)}">실행 이력 확인</button>`;}
    }
    const upcoming=$('.upcoming');
    if(upcoming){
      const order=job=>job.next?.startsWith('오늘')?0:job.next?.startsWith('내일')?1:2;
      const scheduled=jobs.filter(job=>job.triggerType!=='depends_on').sort((left,right)=>order(left)-order(right));
      upcoming.innerHTML=scheduled.slice(0,3).map(job=>`<article class="upcoming-item"><small class="muted">다음 실행</small><p class="upcoming-time">${escape(job.next)}<small>${escape(job.trigger)}</small></p><a class="name-link" href="jobs.html?selected=${encodeURIComponent(job.name)}" data-inspect="${escape(job.name)}">${escape(job.name)}</a></article>`).join('')||'<p class="muted">예약된 작업이 없습니다.</p>';
    }
    filterJobs();
  }

  function filterJobs() {
    if (page !== 'jobs') return;
    const query = $('#job-search').value.trim().toLowerCase();
    let count = 0;
    $$('[data-job-row]').forEach(row => {
      row.hidden = !(row.dataset.name.toLowerCase().includes(query) && (selectedJobFilter === 'all' || row.dataset.state === selectedJobFilter));
      if (!row.hidden) count += 1;
    });
    $('[data-visible-count]').textContent = count;
    $('#search-empty').hidden = count > 0;
    $('.table-wrap').hidden = count === 0;
    saveInventoryState();
    renderInspector();
  }

  function renderJobDetail() {
    if (page !== 'job-detail' || !currentJob) return;
    $$('[data-job-key]').forEach(element => {
      const key = element.dataset.jobKey;
      if (key === 'result') element.innerHTML = badge(currentJob.state);
      else if (key === 'success') element.innerHTML = `${escape(currentJob.success)}${currentJob.success === '—' ? '' : '<small>%</small>'}`;
      else element.textContent = currentJob[key];
    });
    $('[data-breadcrumb-title]').textContent = currentJob.name;
    $('[data-page-description]').textContent = `예약 작업 · ${currentJob.trigger} · Asia/Seoul`;
    $('[data-job-edit-link]').href = `job-new.html?edit=${encodeURIComponent(currentJob.name)}`;
    $('[data-current-job-run]').disabled = currentJob.state === 'running';
    const alert = $('#job-detail-alert');
    alert.hidden = currentJob.state !== 'failed';
    alert.innerHTML = `<div class="alert error">${icon('TriangleAlert')}<div><strong>마지막 실행이 실패했습니다.</strong><span class="alert-detail">종료 코드 1 · ${escape(currentJob.last)}</span></div></div>`;
    const dependencies = $('[data-job-dependencies]');
    const upstream=job=>job.triggerType==='depends_on'?String(job.triggerValue||job.expr).split(',').map(name=>name.trim()).filter(Boolean):[];
    const parents=upstream(currentJob),children=jobs.filter(job=>upstream(job).includes(currentJob.name)).map(job=>job.name);
    dependencies.innerHTML=[['선행 작업',parents],['후속 작업',children]].filter(([,names])=>names.length).map(([label,names])=>`<span class="muted">${label}</span><br>${names.map(name=>`<a class="text-link" href="job-detail.html?name=${encodeURIComponent(name)}">${escape(name)}</a>`).join('<br>')}`).join('<br>')||'<span class="muted" style="font-size:11px">연결된 작업 없음</span>';
    renderRuns();
  }

  function renderRuns() {
    if(page!=='job-detail'||!currentJob)return;
    const rows=getJobRuns(currentJob),presentation=window.SupervisorRunPresentation;
    if(!rows.length){$('#run-table tbody').innerHTML='<tr><td colspan="5"><div class="empty"><h2>아직 실행 이력이 없습니다</h2><p>작업이 실행되면 경로와 상세 로그가 표시됩니다.</p></div></td></tr>';return;}
    $('#run-table tbody').innerHTML=rows.map((run,index)=>`<tr><td><button type="button" class="button quiet small" data-expand-run="run-${index}" aria-controls="run-${index}" aria-expanded="${index===0}">${icon('ChevronRight')}<span class="mono">${escape(run.date)}</span></button>${run.snapshot?`<span class="cell-sub">#${String(run.snapshot.sequence).padStart(3,'0')} · ${escape(run.snapshot.versionLabel)}</span>`:''}</td><td>${badge(run.state)}</td><td class="run-result-cell"><strong>${escape(run.summary?.outcome||run.trigger)}</strong><span>${run.summary?`${run.summary.executed}개 단계 실행`:'이전 요약 기록'}</span></td><td class="number">${escape(run.duration)}</td><td class="number">${run.summary?.exitCode??(run.state==='failed'?1:run.state==='succeeded'?0:'—')}</td></tr><tr class="run-detail" id="run-${index}" ${index===0?'':'hidden'}><td colspan="5">${run.snapshot?presentation.renderPath(run.snapshot):''}${runLinks(currentJob,run)}<small class="recent-run-id">${escape(run.id)}</small></td></tr>`).join('');
  }

  function runJob(name, {isolated=false}={}) {
    const job = jobs.find(item => item.name === name);
    if (!job || job.state === 'running' || ['offline','loading'].includes(activePreviewState)) return;
    if(window.SupervisorWorkflowRuns){return window.SupervisorWorkflowRuns.start({job:{...job,id:job.flowId||job.name},workflow:job.workflow});}
    const shouldFocusInspector=Boolean(document.activeElement?.closest('#detail-inspector'));
    job.state = 'running'; job.last = isolated?'방금 · 단독 실행':'방금 · 수동 실행';
    extraRuns[name] = [{ id: `manual-${Date.now()}`, date: '10.26 14:03:25', state: 'running', duration: '실행 중', trigger: isolated?'단독 실행 · 선행·후속 제외':'수동', executionMode:isolated?'isolated':'normal', skipUpstream:isolated, propagate:!isolated }, ...(extraRuns[name] || [])];
    refreshViews();
    if(shouldFocusInspector) $('#inspector-title')?.focus({preventScroll:true});
    showToast(`${name} ${isolated?'단독':'수동'} 실행을 시작했습니다.`, '실행 이력', () => isInventoryPage ? openInspector(name,'overview',true) : renderJobDetail());
    return extraRuns[name][0];
  }

  function showMenu(anchor, kind, name) {
    const menu = $('#context-menu');
    menuReturnFocus = anchor;
    const safe = escape(name);
    menu.setAttribute('role', 'menu');
    menu.setAttribute('aria-label',`${name} 추가 동작`);
    menu.classList.toggle('process-menu',kind==='process');
    if (kind === 'process') {
      const process = processes.find(item => item.name === name);
      if(!process)return;
      const availability=processActionAvailability(process);
      const item=(action,label,iconName,description,reason='',isDanger=false)=>`<button role="menuitem" type="button" ${isDanger?'class="red"':''} data-process-action="${action}" data-name="${safe}" aria-disabled="${Boolean(reason)}" aria-label="${label}" ${reason?`title="${escape(reason)}"`:''}>${icon(iconName)}<span><strong>${label}</strong><small>${escape(reason||description)}</small></span></button>`;
      menu.innerHTML=`${process.mode==='direct'?item('rolling','순차 재시작…','RefreshCw','새 인스턴스 준비 후 하나씩 교체',availability.rollingReason):''}${item('convert','관리 방식 전환…','ArrowRightLeft',process.mode==='direct'?'직접 관리 → 시스템 등록':'시스템 등록 → 직접 관리',availability.convertReason)}<hr>${availability.canForceStop?item('force-stop','강제 종료…','Square','정상 종료를 기다리지 않고 즉시 종료','',true):''}${item('remove','등록 삭제…','Trash2','실행 파일과 보관된 로그는 유지',availability.removeReason,true)}`;
    } else {
      menu.innerHTML = `<button role="menuitem" type="button" data-run-job-single="${safe}">${icon('Play')}이 작업만 실행</button><button role="menuitem" type="button" data-job-workflow="${safe}">${icon('ArrowRightLeft')}흐름 편집</button><hr><button role="menuitem" class="red" type="button" data-remove-job="${safe}">${icon('Trash2')}작업 삭제</button>`;
    }
    menu.hidden = false;
    const rect = anchor.getBoundingClientRect();
    menu.style.left = `${Math.max(8, Math.min(rect.right - menu.offsetWidth, innerWidth - menu.offsetWidth - 8))}px`;
    menu.style.top = `${Math.min(rect.bottom + 5, innerHeight - menu.offsetHeight - 45)}px`;
    $('[role="menuitem"]', menu).focus();
  }

  function renderLogs() {
    if(logExplorer) logExplorer.render();
  }

  function initializeLogExperience() {
    if(page!=='logs')return;
    $('[data-page-description]').textContent='프로세스와 작업의 기록을 구분하고, 주의가 필요한 로그부터 확인합니다.';
    logExplorer=new window.SupervisorLogExplorer({
      root:$('#normal-view'),records:fixtures.logs,processes,jobs,archiveJobs:seed.jobs,icon,
      firstDate:fixtures.firstDate,lastDate:fixtures.lastDate,snapshot:fixtures.snapshot,
      saved:read('log-explorer-v2',null),save:state=>write('log-explorer-v2',state),notify:showToast,
      onSelection:({kind,source,name})=>{const link=$('[data-log-detail-link]');link.hidden=!source;if(!source)return;link.innerHTML=icon('ArrowLeft')+(kind==='job'?'작업으로':'프로세스 개요로');link.href=kind==='job'?`jobs.html?selected=${encodeURIComponent(name)}`:`index.html?selected=${encodeURIComponent(source)}&tab=overview`;}
    });
  }

  function updateCounts() {
    $$('[data-nav-count="processes"]').forEach(element => { element.textContent = processes.length; });
    $$('[data-nav-count="jobs"]').forEach(element => { element.textContent = jobs.length; });
  }

  const initialDescription = $('[data-page-description]').textContent;
  function setPreviewState(state) {
    activePreviewState = state;
    const normal = $('#normal-view');
    const region = $('#state-view');
    $$('.partial-warning').forEach(element => element.remove());
    normal.hidden = !['normal', 'partial'].includes(state) || (page==='jobs'&&document.body.dataset.jobView&&document.body.dataset.jobView!=='list');
    if(page==='jobs'&&$('#job-flow-root'))$('#job-flow-root').hidden=!['normal','partial'].includes(state);
    region.hidden = ['normal', 'partial'].includes(state);
    $('[data-page-description]').textContent = initialDescription;
    const count = $('[data-page-count]');
    if (count) count.textContent = state === 'empty' ? '00' : ['offline', 'loading'].includes(state) ? '—' : String(page === 'jobs' ? jobs.length : processes.length).padStart(2, '0');
    const isUnavailable = ['offline', 'loading'].includes(state);
    $$('.head-actions button, .head-actions a').forEach(element => {
      if (element.tagName === 'BUTTON') {
        if (isUnavailable && !('wasDisabled' in element.dataset)) element.dataset.wasDisabled = String(element.disabled);
        element.disabled = isUnavailable || element.dataset.wasDisabled === 'true';
        if (!isUnavailable) delete element.dataset.wasDisabled;
      } else {
        if (isUnavailable) { if (element.hasAttribute('href')) element.dataset.originalHref = element.getAttribute('href'); element.removeAttribute('href'); element.setAttribute('aria-disabled', 'true'); }
        else if (element.dataset.originalHref) { element.href = element.dataset.originalHref; delete element.dataset.originalHref; element.removeAttribute('aria-disabled'); }
      }
    });
    $('[data-connection-label]').textContent = state === 'offline' ? '연결 끊김' : state === 'loading' ? '연결 확인 중' : '이 Mac에 연결됨';
    $('[data-connection-dot]').className = `dot ${state === 'offline' ? 'red' : state === 'loading' ? 'amber' : 'green'}`;
    $('.host-sub').textContent = state === 'offline' ? '마지막 확인 14:03:24' : state === 'loading' ? '데이터를 확인하고 있습니다' : '로컬 런타임 · 정상';
    if (state === 'normal') { renderProcesses(); renderProcessDetail(); renderJobs(); renderJobDetail(); renderLogs(); }
    if (state === 'partial') ($('.inventory-pane') || normal).insertAdjacentHTML('afterbegin', `<div class="alert partial-warning">${icon('TriangleAlert')}<div><strong>일부 데이터를 불러오지 못했습니다.</strong><span class="alert-detail">마지막으로 확인한 내용을 표시합니다.</span></div><button type="button" class="button small" data-retry-state>다시 시도</button></div>`);
    if (state === 'empty') {
      const isProcess = page === 'processes';
      $('[data-page-description]').textContent = page === 'logs' ? '현재 표시할 로그가 없습니다.' : `등록된 ${isProcess ? '프로세스' : '작업'}가 없습니다.`;
      region.innerHTML = `<div class="state-message">${icon(page === 'logs' ? 'Terminal' : isProcess ? 'Layers3' : 'Clock3')}<h2>${page === 'logs' ? '아직 보관된 로그가 없습니다' : isProcess ? '첫 프로세스를 등록하세요' : '첫 작업을 예약하세요'}</h2><p>${isProcess ? '실행 명령을 등록하면 상태와 자원 사용량을 확인할 수 있습니다.' : page === 'logs' ? '프로세스 또는 작업을 실행하면 출력과 실행 기록이 여기에 표시됩니다.' : '반복 실행할 명령과 일정을 등록하면 실행 결과를 여기서 확인할 수 있습니다.'}</p><a class="button primary" href="${page === 'jobs' ? 'job-new.html' : 'process-new.html'}">${icon('Plus')}${page === 'jobs' ? '작업 추가' : '프로세스 추가'}</a></div>`;
    }
    if (state === 'offline') {
      $('[data-page-description]').textContent = '연결을 확인할 수 없습니다. 표시된 정보는 최신 상태가 아닐 수 있습니다.';
      region.innerHTML = `<div class="state-message">${icon('Server')}<h2>로컬 런타임에 연결할 수 없습니다</h2><p>마지막 연결은 14:03:24입니다.<br>런타임 상태를 확인한 뒤 다시 시도하세요.</p><button type="button" class="button" data-retry-state>${icon('RefreshCw')}다시 연결</button></div>`;
    }
    if (state === 'loading') {
      $('[data-page-description]').textContent = '로컬 런타임에서 최신 상태를 확인하고 있습니다.';
      region.innerHTML = `<div class="table-wrap" role="status" aria-label="불러오는 중"><table class="data-table"><tbody>${Array.from({length: 5}, () => `<tr>${Array.from({length: 3}, () => '<td><span class="skeleton-line"></span><span class="skeleton-line short"></span></td>').join('')}</tr>`).join('')}</tbody></table></div>`;
    }
  }

  function updateFormPreview() {
    const form = $('#process-form') || $('#job-form');
    if (!form) return;
    const data = new FormData(form);
    $('[data-command-preview]').textContent = executablePicker?.resolvedCommand() || data.get('command') || '실행 명령을 입력하세요';
    $('[data-command-caption]').textContent = data.get('name') || (page === 'job-new' ? '새 작업' : '새 프로세스');
    if (page === 'process-new') $('[data-cwd-preview]').textContent = `작업 디렉터리: ${data.get('cwd') || '기본값'}`;
    if (page === 'job-new') updateSchedulePreview(data);
  }

  function updateSchedulePreview(data) {
    const type = data.get('triggerType');
    const value = String(data.get('triggerValue')).trim();
    const description = $('[data-schedule-description]');
    const preview = $('#schedule-preview');
    if (type === 'depends_on') {
      description.textContent = `${value || '선택한 작업'} 완료 후 실행합니다.`;
      preview.innerHTML = '<li>선행 작업 완료 시 실행</li>';
    } else if (type === 'interval') {
      const seconds = Number(value);
      description.textContent = Number.isFinite(seconds) && seconds > 0 ? `${seconds}초 간격으로 실행합니다.` : '실행 간격을 입력하세요.';
      preview.innerHTML = Number.isFinite(seconds) && seconds > 0 ? Array.from({length: 3}, (_, index) => `<li>${new Date(Date.UTC(2026, 9, 26, 5, 3, 24) + seconds * 1000 * (index + 1)).toLocaleString('ko-KR', {timeZone: 'Asia/Seoul', hour12: false, month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit'})}</li>`).join('') : '<li>간격을 확인해 주세요</li>';
    } else if (type === 'one_shot') {
      description.textContent = '지정한 시각에 한 번 실행합니다.';
      preview.innerHTML = `<li>${escape(value.replace('T', ' ')) || '실행 시각을 입력하세요'}</li>`;
    } else {
      const known = value === '0 2 * * *';
      description.textContent = known ? '매일 오전 2시에 실행합니다.' : '입력한 cron 일정에 따라 실행합니다.';
      preview.innerHTML = known ? '<li>10.27 <span>화요일</span> 02:00</li><li>10.28 <span>수요일</span> 02:00</li><li>10.29 <span>목요일</span> 02:00</li>' : '<li>일정 확인 후 실행 시각을 표시합니다.</li>';
    }
  }

  const isOutsideDialog = (dialog, event) => {
    const bounds = dialog.getBoundingClientRect();
    return event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom;
  };
  let pressedBackdropDialog = null;
  document.addEventListener('pointerdown', event => {
    const dialog = event.target;
    pressedBackdropDialog = event.button === 0 && dialog instanceof HTMLDialogElement && dialog.open && isOutsideDialog(dialog, event) ? dialog : null;
  }, true);
  document.addEventListener('pointercancel', () => { pressedBackdropDialog = null; }, true);
  document.addEventListener('click', event => {
    const dialog = pressedBackdropDialog;
    pressedBackdropDialog = null;
    if (!dialog?.open || event.target !== dialog || !isOutsideDialog(dialog, event)) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    dialog.close();
  }, true);

  document.addEventListener('dblclick', event => {
    const row=event.target.closest('[data-job-row]');
    if(page!=='jobs'||!row||event.target.closest('button, a, input, select, textarea')||event.metaKey||event.ctrlKey||event.shiftKey)return;
    event.preventDefault();
    window.dispatchEvent(new CustomEvent('supervisor-job-workflow-open',{detail:{name:row.dataset.name}}));
  });

  document.addEventListener('click', event => {
    const target = event.target.closest('button, a');
    if (!target) {
      const row=event.target.closest('[data-process-row], [data-job-row]');
      if(row && isInventoryPage) openInspector(row.dataset.name,'overview');
      if (!event.target.closest('#context-menu')) $('#context-menu').hidden = true;
      return;
    }
    if(target.getAttribute('aria-disabled')==='true'){event.preventDefault();return;}
    if(target.matches('[data-inspect]') && !event.metaKey && !event.ctrlKey && !event.shiftKey){event.preventDefault();openInspector(target.dataset.inspect,target.dataset.inspectTab || 'overview',event.detail===0);return;}
    if(target.matches('[data-open-editor]')){event.preventDefault();openEditor(target.dataset.openEditor);return;}
    if(target.matches('[data-edit-process]')){event.preventDefault();$('#context-menu').hidden=true;openEditor('process',target.dataset.editProcess);return;}
    if(target.matches('[data-view-folder]')){event.preventDefault();new window.SampleFolderDialog({path:target.dataset.viewFolder,icon});return;}
    if(target.matches('[data-job-connections]')){event.preventDefault();window.dispatchEvent(new CustomEvent('supervisor-job-connections-open',{detail:{name:target.dataset.jobConnections}}));return;}
    if(target.matches('[data-run-job-single]')){event.preventDefault();window.SupervisorJobWorkspace.runSingle(target.dataset.runJobSingle);return;}
    if(target.matches('[data-job-run]')&&page==='jobs'&&!event.metaKey&&!event.ctrlKey&&!event.shiftKey){event.preventDefault();window.dispatchEvent(new CustomEvent('supervisor-job-workflow-open',{detail:{name:target.dataset.jobRun,runId:target.dataset.runId,runView:target.dataset.runView}}));return;}
    if(target.matches('[data-job-workflow]')){event.preventDefault();window.dispatchEvent(new CustomEvent('supervisor-job-workflow-open',{detail:{name:target.dataset.jobWorkflow}}));return;}
    if(target.matches('[data-edit-job], [data-job-edit-link]')){event.preventDefault();$('#context-menu').hidden=true;openEditor('job',target.dataset.editJob || currentJob.name);return;}
    if(target.matches('[data-close-editor]')){$('#editor-dialog').close();return;}
    if(target.matches('[data-close-inspector]')){closeInspector();return;}
    if(target.matches('[data-move-inspector]')){moveInspector(Number(target.dataset.moveInspector));return;}
    if(target.matches('[data-inspector-tab]')){inventoryState.tab=target.dataset.inspectorTab;saveInventoryState();renderInspector();$(`[data-inspector-tab="${inventoryState.tab}"]`)?.focus({preventScroll:true});return;}
    if(target.matches('[data-reveal-selection]')){const search=$(`#${page==='processes'?'process':'job'}-search`);search.value='';$(`[data-${page==='processes'?'process':'job'}-filter="all"]`).click();return;}
    if (target.matches('[data-toggle-theme]')) setTheme(document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark', true);
    if (target.matches('[data-design-notes]')) $('#design-dialog').showModal();
    if (target.matches('[data-close-dialog]')) target.closest('dialog').close();
    if (target.id === 'dialog-confirm') { $('#action-dialog').close(); if (dialogAction) dialogAction(); dialogAction = null; }
    if (target.matches('[data-reset-demo]')) {
      try{Object.keys(sessionStorage).filter(key=>key.startsWith('msv-mock-')).forEach(key=>sessionStorage.removeItem(key));}catch{}
      location.href=location.pathname;
    }
    if (target.matches('[data-process-filter]')) {
      selectedProcessFilter = target.dataset.processFilter;
      $$('[data-process-filter]').forEach(button => button.setAttribute('aria-pressed', String(button === target)));
      filterProcesses();
    }
    if (target.matches('[data-clear-process-filters]')) { $('#process-search').value = ''; $('[data-process-filter="all"]').click(); $('#process-search').focus(); }
    if (target.matches('[data-job-filter]')) {
      selectedJobFilter = target.dataset.jobFilter;
      $$('[data-job-filter]').forEach(button => button.setAttribute('aria-pressed', String(button === target)));
      filterJobs();
    }
    if (target.matches('[data-clear-job-filters]')) { $('#job-search').value = ''; $('[data-job-filter="all"]').click(); $('#job-search').focus(); }
    if (target.matches('[data-process-menu], [data-current-process-menu]')) showMenu(target, 'process', target.dataset.processMenu || currentProcess.name);
    else if (target.matches('[data-job-menu], [data-current-job-menu]')) showMenu(target, 'job', target.dataset.jobMenu || currentJob.name);
    else if (!target.closest('#context-menu')) $('#context-menu').hidden = true;
    if (target.matches('[data-process-action]')) { $('#context-menu').hidden = true; performProcessAction(target.dataset.name || currentProcess.name, target.dataset.processAction); }
    if (target.matches('[data-run-job], [data-current-job-run]')) runJob(target.dataset.runJob || currentJob.name);
    if (target.matches('[data-remove-job]')) {
      const name = target.dataset.removeJob;
      confirmAction(`${name} 삭제`, `<p><code>${escape(name)}</code>의 예약을 삭제합니다. 이후 예약 실행은 발생하지 않습니다.</p><p>이 동작은 목업 데이터에만 적용됩니다.</p>`, '삭제', () => {
        jobs = jobs.filter(job => job.name !== name); persist();
        if (page === 'job-detail') location.href = 'jobs.html';
        else {if(inventoryState.selected===name){inventoryState.selected=null;saveInventoryState();} refreshViews();showToast(`${name} 작업을 삭제했습니다.`);}
      }, true);
    }
    if (target.matches('[data-expand-run]')) {
      const row = document.getElementById(target.dataset.expandRun);
      row.hidden = !row.hidden;
      target.setAttribute('aria-expanded', String(!row.hidden));
    }
    if (target.matches('[data-cancel-run]')) {
      const job=jobs.find(item=>item.name===(target.dataset.jobName || currentJob.name));
      const run = (extraRuns[job.name] || []).find(item => item.id === target.dataset.cancelRun);
      if (run) {run.state='cancelled';run.duration='—';job.state='cancelled';job.last='방금 · 취소됨';refreshViews();showToast('수동 실행을 취소했습니다. 다음 예약은 유지됩니다.');}
    }
    if (target.matches('[data-refresh-runtime]')) showToast('목업: 로컬 런타임 상태를 새로 확인했습니다.');
    if (target.matches('[data-retry-state]')) { $('#preview-state').value = 'normal'; setPreviewState('normal'); showToast('목업: 정상 연결 상태로 돌아왔습니다.'); }
    if (target.matches('[data-copy]')) {
      if (navigator.clipboard?.writeText) navigator.clipboard.writeText(target.dataset.copy).then(() => showToast('경로를 복사했습니다.')).catch(() => showToast(`복사할 경로: ${target.dataset.copy}`));
      else showToast(`복사할 경로: ${target.dataset.copy}`);
    }
  });

  document.addEventListener('keydown', event => {
    if (event.key === '/' && !event.metaKey && !event.ctrlKey && !event.altKey && !event.target.matches('input, textarea, select') && !document.querySelector('dialog[open]')) {
      const search = page==='jobs'&&document.body.dataset.jobView&&document.body.dataset.jobView!=='list'?$('.flow-search input'):$('input[type="search"]');
      if (search) { event.preventDefault(); search.focus(); }
    }
    if(event.key==='Escape' && !document.querySelector('dialog[open]')) {
      if(page==='jobs'&&document.body.dataset.jobView&&document.body.dataset.jobView!=='list'){window.dispatchEvent(new CustomEvent('supervisor-job-flow-clear'));return;}
      if(!$('#context-menu').hidden){$('#context-menu').hidden=true;menuReturnFocus?.focus({preventScroll:true});}
      else if(isInventoryPage) closeInspector();
    }
    if(isInventoryPage && ['ArrowUp','ArrowDown'].includes(event.key) && event.target.matches('[data-inspect]')) {
      event.preventDefault();const rows=visibleInventoryRows();const index=rows.findIndex(row=>row.dataset.name===event.target.dataset.inspect);const next=rows[index+(event.key==='ArrowDown'?1:-1)];
      if(next){openInspector(next.dataset.name,inventoryState.tab);$('[data-inspect]',next)?.focus({preventScroll:true});next.scrollIntoView({block:'nearest'});}
    }
    if(['ArrowLeft','ArrowRight'].includes(event.key) && event.target.matches('[data-inspector-tab]')) {
      event.preventDefault();const tabs=$$('[data-inspector-tab][role="tab"]');const next=tabs[(tabs.indexOf(event.target)+1)%tabs.length];next.click();
    }
    if (!$('#context-menu').hidden && ['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      const items = $$('[role="menuitem"]', $('#context-menu'));
      const index = items.indexOf(document.activeElement);
      event.preventDefault();
      items[event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length].focus();
    }
  });
  window.addEventListener('resize', () => { $('#context-menu').hidden = true; updateInspectorInteractivity(); });
  window.addEventListener('popstate',()=>{
    if(!isInventoryPage) return;
    const query=new URLSearchParams(location.search);
    inventoryState.selected=query.get('selected');inventoryState.tab=query.get('tab') || 'overview';
    if(!shouldRetainFiltersOnBack){inventoryState.query=query.get('q') || '';inventoryState.filter=query.get('filter') || 'all';$(`#${page==='processes'?'process':'job'}-search`).value=inventoryState.query;if(page==='processes')selectedProcessFilter=inventoryState.filter;else selectedJobFilter=inventoryState.filter;}
    shouldRetainFiltersOnBack=false;
    $$(`[data-${page==='processes'?'process':'job'}-filter]`).forEach(button=>button.setAttribute('aria-pressed',String(button.dataset[page==='processes'?'processFilter':'jobFilter']===inventoryState.filter)));
    if(page==='processes')filterProcesses();else filterJobs();
  });
  document.addEventListener('input',event=>{if(event.target.id==='inspector-instance-count'){const value=Number(event.target.value);$('button[type="submit"]',event.target.form).disabled=!Number.isInteger(value)||value<1||value>20||value===(currentProcess.instances||1);}});
  document.addEventListener('submit',event=>{if(event.target.matches('[data-inspector-scale]')){event.preventDefault();const count=Number(event.target.elements.instances.value);if(!Number.isInteger(count)||count<1||count>20)return;currentProcess.instances=count;refreshViews();showToast(`목표 인스턴스 ${count}개를 적용했습니다.`);}});
  document.addEventListener('scroll', () => { $('#context-menu').hidden = true; }, true);
  $('#preview-state').addEventListener('change', event => setPreviewState(event.target.value));
  $('#process-search')?.addEventListener('input', filterProcesses);
  $('#job-search')?.addEventListener('input', filterJobs);
  $$('input[name="theme"]').forEach(input => input.addEventListener('change', () => setTheme(input.value, true)));

  $('#scale-form')?.addEventListener('submit', event => {
    event.preventDefault();
    const count = Number($('#instance-count').value);
    if (!Number.isInteger(count) || count < 1 || count > 20 || !currentProcess) return;
    const previous = currentProcess.instances || 1;
    confirmAction('인스턴스 수 변경', `<p><code>${escape(currentProcess.name)}</code>의 목표 인스턴스를 ${previous}개에서 ${count}개로 변경합니다.</p><p>이 동작은 목업 데이터에만 적용됩니다.</p>`, '변경', () => {
      currentProcess.instances = count; persist(); renderProcessDetail();
      $('#operation-result').hidden = false;
      $('#operation-result').innerHTML = `<div class="alert info">${icon('CheckCircle2')}목업: 목표 인스턴스 ${count}개를 적용했습니다.</div>`;
    });
  });

  $('#process-form')?.addEventListener('input', updateFormPreview);
  $('#process-form')?.addEventListener('submit', event => {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const name = String(data.get('name')).trim();
    const command = (executablePicker?.resolvedCommand() || String(data.get('command'))).trim();
    if (!name || !command) return;
    if (processes.some(process => process.name === name)) { form.elements.name.setCustomValidity('이미 사용 중인 이름입니다.'); form.elements.name.reportValidity(); return; }
    const mode = data.get('mode');
    const selectedExecutable=executablePicker?.selection();
    const profileOverrides=selectedExecutable?{runtime:`${selectedExecutable.label}${selectedExecutable.version?' '+selectedExecutable.version:''}`,executable:executablePicker.bindingSnapshot()}:{};
    processes.push({ name, command, cwd: String(data.get('cwd')).trim(), profileOverrides, mode, modeLabel: mode === 'direct' ? '직접 관리' : '시스템 등록', state: 'stopped', label: '중지됨', pid: '—', cpu: '—', memory: '—', uptime: '—', restarts: 0 });
    persist(); location.href = `index.html?selected=${encodeURIComponent(name)}`;
  });
  $('#process-form input[name="name"]')?.addEventListener('input', event => { event.target.setCustomValidity(''); });

  const jobForm = $('#job-form');
  if (jobForm) {
    const editing = jobs.find(job => job.name === params.get('edit'));
    const configureTrigger = () => {
      const type = jobForm.elements.triggerType.value;
      const fields = { cron: ['cron 표현식','0 2 * * *','매일 오전 2시에 실행합니다. 시간대는 Asia/Seoul입니다.','text'], interval: ['실행 간격 (초)','900','1 이상의 초 단위 간격을 입력하세요.','number'], one_shot: ['실행 시각','2026-10-27T02:00','Asia/Seoul 기준 시각입니다.','datetime-local'], depends_on: ['선행 작업 이름','nightly-backup','여러 작업은 쉼표로 구분합니다.','text'] }[type];
      $('#trigger-value-label').textContent = fields[0]; $('#trigger-value-hint').textContent = fields[2];
      const input = jobForm.elements.triggerValue;
      input.type = fields[3]; input.min = type === 'interval' ? '1' : ''; input.value = fields[1];
      updateFormPreview();
    };
    jobForm.elements.triggerType.addEventListener('change', configureTrigger);
    jobForm.addEventListener('input', updateFormPreview);
    if (editing) {
      jobForm.elements.name.value = editing.name; jobForm.elements.name.readOnly = true;
      jobForm.elements.command.value = editing.command; jobForm.elements.triggerType.value = editing.triggerType; configureTrigger();
      jobForm.elements.triggerValue.value = editing.triggerValue; jobForm.elements.policy.value = editing.policy;
      $('[data-job-editor-title]').textContent = '작업 수정';
      $('[data-breadcrumb-title]').textContent = '작업 수정';
      $('[data-job-submit]').textContent = '변경 저장';
    }
    jobForm.addEventListener('submit', event => {
      event.preventDefault();
      const data = new FormData(jobForm);
      const name = String(data.get('name')).trim();
      const command = (executablePicker?.resolvedCommand() || String(data.get('command'))).trim();
      if (!name || !command) return;
      if (!editing && jobs.some(job => job.name === name)) { jobForm.elements.name.setCustomValidity('이미 사용 중인 이름입니다.'); jobForm.elements.name.reportValidity(); return; }
      const triggerType = data.get('triggerType');
      const value = String(data.get('triggerValue')).trim();
      const policy = data.get('policy');
      const trigger = triggerType === 'cron' ? value === '0 2 * * *' ? '매일 02:00' : '사용자 지정 일정' : triggerType === 'interval' ? `${value}초마다` : triggerType === 'depends_on' ? '선행 작업 완료 후' : '한 번 실행';
      const next = triggerType === 'depends_on' ? '선행 작업 대기' : triggerType === 'cron' && value === '0 2 * * *' ? '내일 02:00' : '일정 확인 대기';
      const record = { ...(editing || { state: 'pending', label: '대기 중', last: '아직 실행되지 않음', success: '—', isNewMock: true }), name, command, executable:executablePicker?.bindingSnapshot(), triggerType, triggerValue: value, expr: value, policy, policyLabel: {skip:'건너뛰기',queue:'대기열에 추가',parallel:'동시 실행'}[policy], trigger, next };
      if (editing) Object.assign(editing, record); else jobs.push(record);
      persist(); location.href = `jobs.html?selected=${encodeURIComponent(name)}`;
    });
    jobForm.elements.name.addEventListener('input', event => { event.target.setCustomValidity(''); });
  }

  const standaloneForm=$('#process-form')||$('#job-form');
  if(standaloneForm&&window.SupervisorExecutablePicker){
    const binding=jobForm?jobs.find(job=>job.name===params.get('edit'))?.executable:null;
    executablePicker=new window.SupervisorExecutablePicker({input:standaloneForm.elements.command,icon,cwd:()=>standaloneForm.elements.cwd?.value||'',binding,notify:showToast});
    standaloneForm.elements.command.addEventListener('executablechange',updateFormPreview);
    standaloneForm.elements.cwd?.addEventListener('input',()=>executablePicker.schedule());
  }

  initializeInventory();
  window.addEventListener('supervisor-workflow-history-ready',()=>{renderJobs();renderJobDetail();renderInspector();});
  initializeLogExperience();
  updateCounts();
  renderProcesses();
  renderProcessDetail();
  renderJobs();
  renderJobDetail();
  renderLogs();
  updateFormPreview();
  renderInspector();

  if(['jobs','job-detail'].includes(page))window.SupervisorJobWorkspace={
    snapshot:()=>({jobs:structuredClone(jobs),processes:processes.map(({name,state})=>({name,state})),selected:inventoryState.selected,previewState:activePreviewState}),
    icon,
    notify:showToast,
    async runSingle(name){
      const workflowJob=jobs.find(item=>item.name===name);
      if(workflowJob&&window.SupervisorWorkflowRuns){const result=await window.SupervisorWorkflowRuns.start({job:{...workflowJob,id:workflowJob.flowId||workflowJob.name},workflow:workflowJob.workflow});return {state:result.state,detail:'작업 내부 흐름 단독 실행 · 선행·후속 작업 제외'};}
      const run=runJob(name,{isolated:true});
      if(!run)return {state:'failed',detail:'이미 실행 중이거나 현재 실행할 수 없습니다.'};
      await new Promise(resolve=>setTimeout(resolve,750));
      const job=jobs.find(item=>item.name===name);
      if(run.state==='cancelled')return {state:'cancelled',detail:'단독 실행이 취소되었습니다.'};
      Object.assign(run,{state:'succeeded',duration:'1초'});
      Object.assign(job,{state:'succeeded',last:'방금 · 단독 실행'});
      refreshViews();showToast(`${name} 단독 실행 완료 · 후속 작업은 실행하지 않았습니다. (목업)`);
      return {state:'succeeded',detail:'단독 실행 완료 · 선행 조건과 후속 작업 제외 · 목업'};
    },
    beginWorkflowRun(run){
      if(run.scope!=='flow')return;
      const job=jobs.find(item=>(item.flowId||item.name)===run.jobId);if(!job)return;
      const date=new Date(run.startedAt);
      const label=`${String(date.getMonth()+1).padStart(2,'0')}.${String(date.getDate()).padStart(2,'0')} ${date.toLocaleTimeString('ko-KR',{hour12:false})}`;
      job.state='running';job.last='방금 · 흐름 실행';
      extraRuns[job.name]=[{id:run.id,date:label,state:'running',duration:'실행 중',trigger:'흐름 실행 · 선행·후속 작업 제외',workflowRunId:run.id},...(extraRuns[job.name]||[])];
      refreshViews();
    },
    finishWorkflowRun(run){
      if(run.scope!=='flow')return;
      const job=jobs.find(item=>(item.flowId||item.name)===run.jobId);if(!job)return;
      const record=extraRuns[job.name]?.find(item=>item.id===run.id);
      if(record)Object.assign(record,{state:run.state,duration:`${(run.durationMs/1000).toFixed(1)}초`});
      Object.assign(job,{state:run.state,last:`방금 · 흐름 ${run.state==='succeeded'?'완료':run.state==='failed'?'실패':'취소'}`});
      refreshViews();
    },
    async previewStep(jobId,step,input){
      const data=step.data;
      if(data.kind==='process'){
        const process=processes.find(item=>item.name===data.processName);
        if(!process||pendingProcesses.has(data.processName))return {state:'failed',detail:'프로세스가 없거나 다른 동작을 처리 중입니다.'};
        performProcessAction(data.processName,data.operation);
        await new Promise(resolve=>setTimeout(resolve,750));
        const isFailed=process.state==='crashed';
        return {state:isFailed?'failed':'succeeded',detail:`${data.processName} ${{start:'시작',stop:'중지',restart:'재시작'}[data.operation]} ${isFailed?'실패':'완료'} · 후속 단계 실행 안 함 · 목업`};
      }
      if(data.kind==='job')return this.runSingle(jobs.find(job=>(job.flowId||job.name)===data.jobId)?.name);
      await new Promise(resolve=>setTimeout(resolve,600));
      if(data.kind==='condition'&&data.source==='exit_code'){
        const actual=Number(input),expected=Number(data.value);
        const result=data.operator==='equals'?actual===expected:data.operator==='not_equals'?actual!==expected:actual>=expected;
        return {state:'succeeded',detail:`종료 코드 ${actual} → ${result?'참':'거짓'} · 분기 이후 실행 안 함 · 목업`};
      }
      return {state:data.kind==='finish'?data.result:'succeeded',detail:`${data.label} 단독 실행 결과 · 선행·후속 단계 제외 · 목업`};
    },
    setMode(mode){
      $('#normal-view').hidden=mode!=='list'||!['normal','partial'].includes(activePreviewState);
      document.body.dataset.jobView=mode;
      $('.page-head h1').textContent=mode==='flow'?'작업 간 연결':'작업';
      $('[data-page-description]').textContent=mode==='flow'?'작업 사이의 선행·후속 관계를 구성합니다.':'실행 조건과 이력을 한곳에서 확인합니다.';
      updateInspectorInteractivity();
      write('job-view',mode);
      const url=new URL(location.href);if(mode!=='list')url.searchParams.set('view',mode);else url.searchParams.delete('view');history.replaceState(history.state,'',url);
    },
    mode:()=>params.get('view')||read('job-view','list'),
    select(name){inventoryState.selected=name;saveInventoryState();renderInspector();},
    subscribe(listener){window.addEventListener('supervisor-jobs-changed',listener);return ()=>window.removeEventListener('supervisor-jobs-changed',listener);},
    commit(records){
      const renamed=new Map();
      const next=records.map(record=>{
        const current=jobs.find(job=>(job.flowId||job.name)===record.flowId);
        if(current&&current.name!==record.name){renamed.set(current.name,record.name);if(extraRuns[current.name]){extraRuns[record.name]=extraRuns[current.name];delete extraRuns[current.name];}}
        const changedSchedule=!current||current.triggerType!==record.triggerType||current.triggerValue!==record.triggerValue;
        const nextRun=record.triggerType==='depends_on'?'선행 작업 대기':record.triggerType==='interval'?`${record.triggerValue}초 후`:record.triggerType==='one_shot'?record.triggerValue:record.triggerValue==='0 2 * * *'?'내일 02:00':'일정 확인 대기';
        const cronLabels={'0 2 * * *':'매일 02:00','0 0 * * *':'매일 00:00','0 4 * * 0':'매주 일요일 04:00'};
        const seconds=Number(record.triggerValue);
        const interval=seconds%3600===0?`${seconds/3600}시간마다`:seconds%60===0?`${seconds/60}분마다`:`${seconds}초마다`;
        const trigger=record.triggerType==='depends_on'?'선행 작업 완료 후':record.triggerType==='interval'?interval:record.triggerType==='one_shot'?'한 번 실행':cronLabels[record.triggerValue]||'사용자 지정 일정';
        const expr=!changedSchedule&&current?current.expr:record.triggerType==='interval'?`interval · ${record.triggerValue}s`:record.triggerValue;
        return Object.assign(current||{state:'pending',label:'대기 중',last:'아직 실행되지 않음',success:'—',isNewMock:true},record,{trigger,expr,policyLabel:{skip:'건너뛰기',queue:'대기열에 추가',parallel:'동시 실행'}[record.policy],next:changedSchedule?nextRun:current.next});
      });
      jobs=next;
      if(renamed.has(inventoryState.selected))inventoryState.selected=renamed.get(inventoryState.selected);
      currentJob=jobs.find(job=>job.name===currentJob?.name)||jobs[0];
      refreshViews();saveInventoryState();showToast('작업 설정과 의존 관계를 저장했습니다.');
    },
  };
})();
