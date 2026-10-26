/* A searchable key/value view over the same .env field used by import and merge. */
(() => {
  const escape=value=>String(value??'').replace(/[&<>"']/g,char=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'})[char]);
  const related={java:/^(JAVA_HOME|JAVA_TOOL_OPTIONS|JDK_JAVA_OPTIONS|CLASSPATH|PATH)$/,node:/^(NODE_ENV|NODE_OPTIONS|NODE_PATH|UV_THREADPOOL_SIZE|PATH)$/,python:/^(PYTHON.*|VIRTUAL_ENV|PATH)$/,go:/^(GO.*|PATH)$/};

  class EnvironmentEditor {
    constructor({source,icon}) {
      this.source=source;this.icon=icon;this.rows=[];this.mode='table';this.family='';this.isWriting=false;
      this.root=document.createElement('div');this.root.className='environment-editor';
      this.root.innerHTML=`<div class="environment-toolbar"><label class="environment-search">${icon('Search')}<input type="search" aria-label="환경변수 검색" placeholder="이름 또는 값 검색"></label><div class="environment-view-toggle"><button type="button" data-env-mode="table" aria-pressed="true">표</button><button type="button" data-env-mode="raw" aria-pressed="false">.env</button></div></div><div class="environment-runtime" hidden></div><div class="environment-filter"><span data-env-count></span><label><input type="checkbox" data-env-related>실행자 관련만</label></div><p class="environment-error" role="status" hidden></p><div class="environment-table-wrap"><table class="environment-table"><thead><tr><th>이름</th><th>값</th><th><span class="sr-only">삭제</span></th></tr></thead><tbody></tbody></table></div><button type="button" class="environment-add" data-env-add>${icon('Plus')}환경변수 추가</button>`;
      source.before(this.root);source.hidden=true;source.classList.add('environment-raw');
      this.onSource=()=>{if(!this.isWriting)this.readSource();};source.addEventListener('input',this.onSource);
      this.onResize=()=>this.resizeValues();window.addEventListener('resize',this.onResize);
      this.root.addEventListener('input',event=>{
        if(event.target.matches('[data-env-key], [data-env-value]')){const row=this.rows[Number(event.target.dataset.envKey??event.target.dataset.envValue)];row[event.target.hasAttribute('data-env-key')?'key':'value']=event.target.value;this.write();}
        else this.filter();
      });
      this.root.addEventListener('change',()=>this.filter());
      this.root.addEventListener('click',event=>{
        const button=event.target.closest('button');if(!button)return;
        if(button.dataset.envMode)this.setMode(button.dataset.envMode);
        if(button.hasAttribute('data-env-add')){this.root.querySelector('input[type="search"]').value='';this.root.querySelector('[data-env-related]').checked=false;this.rows.push({key:'',value:''});this.renderRows();this.root.querySelector(`[data-env-key="${this.rows.length-1}"]`).focus();}
        if(button.dataset.envRemove!==undefined){this.rows.splice(Number(button.dataset.envRemove),1);this.write();this.renderRows();}
        if(button.hasAttribute('data-env-java-home')){let row=this.rows.find(item=>item.key==='JAVA_HOME');if(row)row.value=this.runtime.homePath;else this.rows.unshift({key:'JAVA_HOME',value:this.runtime.homePath});this.write();this.renderRows();this.renderRuntime();}
      });
      this.readSource();
    }

    readSource() {
      const parsed=window.SupervisorEnvironment.parse(this.source.value);
      if(parsed.error){this.setMode('raw');this.showError(parsed.error);return;}
      this.rows=Object.entries(parsed.environment).map(([key,value])=>({key,value}));this.showError('');this.renderRows();this.renderRuntime();
    }

    write() {
      this.source.value=this.rows.filter(row=>row.key||row.value).map(row=>window.SupervisorEnvironment.serialize({[row.key]:row.value})).join('\n');
      this.isWriting=true;this.source.dispatchEvent(new Event('input',{bubbles:true}));this.isWriting=false;
      const parsed=window.SupervisorEnvironment.parse(this.source.value);this.showError(parsed.error||'');this.filter();this.renderRuntime();
    }

    renderRows() {
      this.root.querySelector('tbody').innerHTML=this.rows.map((row,index)=>`<tr data-env-row="${index}"><td><input data-env-key="${index}" aria-label="환경변수 ${index+1} 이름" value="${escape(row.key)}" placeholder="KEY" spellcheck="false"></td><td><textarea data-env-value="${index}" aria-label="${escape(row.key||'새 환경변수')} 값" rows="1" spellcheck="false" placeholder="값">${escape(row.value)}</textarea></td><td><button type="button" class="icon-button" data-env-remove="${index}" aria-label="${escape(row.key||'새 환경변수')} 삭제">${this.icon('X')}</button></td></tr>`).join('');
      this.filter();
    }

    filter() {
      const query=this.root.querySelector('input[type="search"]').value.toLowerCase();
      const checkbox=this.root.querySelector('[data-env-related]');if(!related[this.family])checkbox.checked=false;
      const onlyRelated=checkbox.checked;let visible=0;
      this.root.querySelectorAll('[data-env-row]').forEach(element=>{const row=this.rows[Number(element.dataset.envRow)];const matches=(!query||`${row.key} ${row.value}`.toLowerCase().includes(query))&&(!onlyRelated||related[this.family]?.test(row.key));element.hidden=!matches;if(matches)visible++;});
      this.root.querySelector('[data-env-count]').textContent=`${visible} / ${this.rows.length}개`;
      this.root.querySelector('[data-env-related]').disabled=!related[this.family];
      this.resizeValues();
    }

    resizeValues() {
      cancelAnimationFrame(this.resizeFrame);
      this.resizeFrame=requestAnimationFrame(()=>this.root.querySelectorAll('tr:not([hidden]) textarea').forEach(input=>{input.style.height='0px';input.style.height=`${Math.min(104,Math.max(34,input.scrollHeight))}px`;}));
    }

    setMode(mode) {
      if(mode==='table'&&window.SupervisorEnvironment.parse(this.source.value).error){this.source.focus();return;}
      this.mode=mode;this.source.hidden=mode!=='raw';
      this.root.querySelector('.environment-table-wrap').hidden=mode==='raw';this.root.querySelector('[data-env-add]').hidden=mode==='raw';
      this.root.querySelector('.environment-filter').hidden=mode==='raw';
      this.root.querySelector('.environment-search').hidden=mode==='raw';
      this.root.querySelectorAll('[data-env-mode]').forEach(button=>button.setAttribute('aria-pressed',String(button.dataset.envMode===mode)));
      if(mode==='table')this.resizeValues();
    }

    showError(message) {const element=this.root.querySelector('.environment-error');element.textContent=message;element.hidden=!message;}

    setRuntime(entry,family) {this.runtime=entry;this.family=family;this.renderRuntime();this.filter();}

    focus() {if(this.mode==='raw')this.source.focus();else this.root.querySelector('input[type="search"]').focus();}

    renderRuntime() {
      const element=this.root.querySelector('.environment-runtime');element.hidden=this.family!=='java';
      if(this.family!=='java')return;
      const home=this.runtime?.homePath,current=this.rows.find(row=>row.key==='JAVA_HOME');
      element.innerHTML=`<p><strong>Java 환경</strong><span>JVM 옵션(-Xmx, -D 등)은 실행 명령에서 관리합니다.</span></p>${home?`<div><code title="${escape(home)}">${escape(home)}</code>${current?.value===home?'<span>JAVA_HOME 연결됨</span>':`<button type="button" data-env-java-home>${current?'선택한 JDK로 JAVA_HOME 변경':'JAVA_HOME 추가'}</button>`}</div>`:'<small>JDK 경로를 선택하면 JAVA_HOME을 연결할 수 있습니다.</small>'}`;
    }

    destroy() {cancelAnimationFrame(this.resizeFrame);window.removeEventListener('resize',this.onResize);this.source.removeEventListener('input',this.onSource);this.source.hidden=false;this.root.remove();}
  }
  window.SupervisorEnvironmentEditor=EnvironmentEditor;
})();
