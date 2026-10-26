/* Installation is explicit; merely opening this dialog only reads release metadata. */
(() => {
  const escape=value=>String(value??'').replace(/[&<>"']/g,char=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'})[char]);
  const families={node:'Node.js',go:'Go',python:'Python',java:'Java · Temurin JDK'};
  const sources={node:['nodejs.org','https://nodejs.org/en/download'],go:['go.dev','https://go.dev/dl/'],python:['Astral python-build-standalone','https://github.com/astral-sh/python-build-standalone'],java:['Eclipse Adoptium','https://adoptium.net/']};
  const phaseLabels={queued:'설치 준비',downloading:'다운로드 중',verifying:'다운로드 파일 검증 중',extracting:'설치 파일 압축 해제 중',registering:'실행자 등록 중',complete:'설치 완료',failed:'설치하지 못했습니다',cancelled:'설치를 취소했습니다'};

  class RuntimeInstaller {
    open(options) {
      this.onInstalled=options.onInstalled;this.notify=options.notify||(()=>{});this.icon=options.icon||(()=> '');
      if(!this.dialog){this.dialog=document.createElement('dialog');this.dialog.className='runtime-install-dialog';this.dialog.setAttribute('aria-label','실행자 설치');document.body.append(this.dialog);this.dialog.addEventListener('close',()=>{if(this.job&&!this.isFinished())this.notify('실행자 설치를 계속 진행합니다. + 버튼에서 진행 상황을 확인하세요.');});}
      if(!this.job||['failed','cancelled'].includes(this.job.phase)||this.isFinished()&&this.family!==options.family){this.job=null;this.family=Object.hasOwn(families,options.family)?options.family:'node';this.catalog=null;}
      this.render();this.dialog.showModal();if(!this.job)this.loadCatalog();
    }

    isFinished() {return ['complete','failed','cancelled'].includes(this.job?.phase);}

    render() {
      this.dialog.innerHTML=`<header class="runtime-install-header"><div><span>새 실행자</span><h2>실행자 설치</h2></div><button type="button" class="icon-button" data-install-close aria-label="설치 창 닫기">×</button></header><div class="runtime-install-body"><div class="runtime-install-form" ${this.job?'hidden':''}><label>실행자<select data-install-family>${Object.entries(families).map(([key,label])=>`<option value="${key}" ${key===this.family?'selected':''}>${label}</option>`).join('')}</select></label><div class="install-version-heading"><label for="install-version">버전</label><label class="install-all"><input type="checkbox" data-install-all>모든 패치 버전</label></div><select id="install-version" data-install-version disabled><option>버전 목록을 불러오는 중…</option></select><div class="install-path-field"><label for="install-path">설치 경로</label><div class="install-path-controls"><textarea id="install-path" data-install-path rows="2" spellcheck="false" placeholder="/Users/you/Library/Application Support/my-supervisor-preview/runtimes/"></textarea><button type="button" class="button" data-install-folder>${this.icon('FolderOpen')}폴더 선택</button></div></div><p class="install-path-hint">새 폴더에 설치합니다. 기존 폴더는 덮어쓰지 않습니다.</p><dl class="install-package"><div><dt>플랫폼</dt><dd data-install-platform>확인 중</dd></div><div><dt>배포처</dt><dd><a href="${sources[this.family][1]}" target="_blank" rel="noopener">${sources[this.family][0]}</a></dd></div><div><dt>다운로드</dt><dd data-install-size>확인 중</dd></div></dl><p class="install-scope-note">전역 PATH를 변경하지 않습니다. 설치된 버전은 이 앱의 실행자 목록에 추가됩니다.</p></div><div class="runtime-install-progress" ${this.job?'':'hidden'}><strong data-install-phase></strong><p data-install-detail></p><progress max="100"></progress><code data-install-destination></code></div><p class="runtime-install-error" role="alert" hidden></p></div><footer class="runtime-install-footer"><span data-install-footer-note>버전과 경로를 확인한 뒤 설치하세요.</span><button type="button" class="button" data-install-cancel>${this.job&&!this.isFinished()?'설치 취소':'취소'}</button><button type="button" class="button primary" data-install-submit ${this.catalog||this.job?.phase==='complete'?'':'disabled'}>${this.job?.phase==='complete'?'실행자로 사용':'설치'}</button></footer>`;
      this.dialog.querySelector('[data-install-close]').onclick=()=>this.dialog.close();
      this.dialog.querySelector('[data-install-family]').onchange=event=>{this.family=event.target.value;this.catalog=null;this.pathEdited=false;this.render();this.loadCatalog();};
      this.dialog.querySelector('[data-install-all]').onchange=()=>this.renderVersions();
      this.dialog.querySelector('[data-install-version]').onchange=()=>this.updateSelection();
      this.dialog.querySelector('[data-install-path]').oninput=()=>{this.pathEdited=true;};
      this.dialog.querySelector('[data-install-folder]').onclick=()=>this.chooseFolder();
      this.dialog.querySelector('[data-install-cancel]').onclick=()=>this.job&&!this.isFinished()?this.cancel():this.dialog.close();
      this.dialog.querySelector('[data-install-submit]').onclick=()=>this.job?.phase==='complete'?this.useInstalled():this.install();
      if(this.catalog){this.renderVersions();this.dialog.querySelector('[data-install-platform]').textContent=this.catalog.platform;}
      if(this.job)this.renderProgress();
    }

    async loadCatalog() {
      const family=this.family;this.pathEdited=false;this.setError('');
      try {
        const response=await fetch(`/__preview__/runtime-catalog?family=${encodeURIComponent(family)}`),data=await response.json();
        if(!response.ok)throw new Error(data.error||'배포 목록을 읽지 못했습니다.');
        if(this.family!==family)return;
        this.catalog=data;this.dialog.querySelector('[data-install-platform]').textContent=data.platform;this.renderVersions();
      } catch(error) {if(this.family===family){this.setError(error.message);this.dialog.querySelector('[data-install-version]').innerHTML='<option>목록을 불러오지 못했습니다</option>';}}
    }

    renderVersions() {
      if(!this.catalog)return;
      const select=this.dialog.querySelector('[data-install-version]'),previous=select.value;
      const all=this.dialog.querySelector('[data-install-all]').checked;
      const lines=new Set();
      const recent=this.catalog.releases.filter(item=>{const line=item.version.split('.').slice(0,['python','go'].includes(this.family)?2:1).join('.');if(lines.has(line))return false;lines.add(line);return true;}).slice(0,6);
      const values=all?this.catalog.releases:recent;
      select.innerHTML=values.map(item=>`<option value="${item.id}">${escape(item.version)}${item.lts?' · LTS':''}</option>`).join('');
      if(values.some(item=>item.id===previous))select.value=previous;
      else if(!all){const lts=values.find(item=>item.lts);if(lts)select.value=lts.id;}
      select.disabled=false;this.dialog.querySelector('[data-install-submit]').disabled=false;this.updateSelection();
    }

    updateSelection() {
      const release=this.selectedRelease();if(!release)return;
      const input=this.dialog.querySelector('[data-install-path]');if(!this.pathEdited)input.value=`${this.catalog.defaultRoot}/${release.version}`;
      this.dialog.querySelector('[data-install-size]').textContent=release.size?`${(release.size/1024/1024).toFixed(1)} MiB`:'다운로드 시 확인';
    }

    selectedRelease() {return this.catalog?.releases.find(item=>item.id===this.dialog.querySelector('[data-install-version]').value);}
    setError(message) {const error=this.dialog.querySelector('.runtime-install-error');error.textContent=message;error.hidden=!message;}

    chooseFolder() {
      const input=this.dialog.querySelector('[data-install-path]'),current=input.value.trim().replace(/\/+$/,'');
      const split=current.lastIndexOf('/'),name=current.slice(split+1)||`${this.family}-${this.selectedRelease()?.version||'runtime'}`;
      new window.SupervisorInstallFolderDialog({path:split>=0?current.slice(0,split)||'/':'',folderName:name,icon:this.icon,onChoose:path=>{input.value=path;this.pathEdited=true;this.setError('');input.focus();}});
    }

    async install() {
      const release=this.selectedRelease(),destination=this.dialog.querySelector('[data-install-path]').value.trim();
      if(!release||!destination.startsWith('/')){this.setError('버전과 설치할 절대 경로를 확인하세요.');return;}
      this.dialog.querySelector('[data-install-submit]').disabled=true;this.setError('');
      try {
        const response=await fetch('/__preview__/runtime-install',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({family:this.family,releaseId:release.id,destination})}),data=await response.json();
        if(!response.ok)throw new Error(data.error||'설치를 시작하지 못했습니다.');
        this.job=data;this.render();this.poll();
      } catch(error){this.setError(error.message);this.dialog.querySelector('[data-install-submit]').disabled=false;}
    }

    async poll() {
      if(!this.job||this.isFinished())return;
      try {
        const response=await fetch(`/__preview__/runtime-install?id=${this.job.id}`,{cache:'no-store'}),data=await response.json();
        if(!response.ok)throw new Error(data.error||'설치 상태를 읽지 못했습니다.');
        this.job=data;this.renderProgress();
        if(this.isFinished()){if(!this.dialog.open)this.notify(`${families[this.family]} ${this.job.version} · ${phaseLabels[this.job.phase]}`);return;}
      } catch(error){if(this.dialog.open)this.setError(error.message);}
      this.timer=setTimeout(()=>this.poll(),650);
    }

    renderProgress() {
      const job=this.job,finished=this.isFinished();
      this.dialog.querySelector('[data-install-phase]').textContent=phaseLabels[job.phase];
      this.dialog.querySelector('[data-install-detail]').textContent=`${families[job.family]} ${job.version}${job.total?` · ${(job.downloaded/1024/1024).toFixed(1)} / ${(job.total/1024/1024).toFixed(1)} MiB`:''}`;
      this.dialog.querySelector('[data-install-destination]').textContent=job.entry?.path||job.destination;
      const progress=this.dialog.querySelector('progress');
      if(job.phase==='complete')progress.value=100;else if(job.total&&job.phase==='downloading')progress.value=job.downloaded/job.total*100;else progress.removeAttribute('value');
      const submit=this.dialog.querySelector('[data-install-submit]');submit.textContent=job.phase==='complete'?'실행자로 사용':'설치';submit.disabled=job.phase!=='complete';
      this.dialog.querySelector('[data-install-cancel]').textContent=finished?'닫기':'설치 취소';
      this.dialog.querySelector('[data-install-footer-note]').textContent=finished?'': '창을 닫아도 설치는 계속됩니다.';
      if(job.error)this.setError(job.error);
    }

    async cancel() {
      try {
        const response=await fetch('/__preview__/runtime-install',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({action:'cancel',id:this.job.id})});
        if(!response.ok)throw new Error('설치 취소 요청을 전달하지 못했습니다. 다시 시도하세요.');
      } catch(error) {this.setError(error.message);}
    }

    useInstalled() {if(this.job?.entry){this.onInstalled?.(this.job.entry);this.dialog.close();}}
  }
  window.SupervisorRuntimeInstaller=new RuntimeInstaller();
})();
