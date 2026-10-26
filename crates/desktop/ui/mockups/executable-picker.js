/* The command stays editable; discovery only replaces its executable token. */
(() => {
  const escape=value=>String(value??'').replace(/[&<>"']/g,char=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'})[char]);

  function commandWords(command) {
    const words=[];let index=0;
    while(index<command.length) {
      while(/\s/.test(command[index]||'')&&index<command.length)index++;
      if(index>=command.length)break;
      const start=index;let value='',quote='';
      for(;index<command.length;index++) {
        const char=command[index];
        if(!quote&&/\s/.test(char))break;
        if(char==='\\'&&quote!=="'"){
          const next=command[index+1];
          if(next!==undefined&&(!quote||'$`"\\\n'.includes(next))){index++;if(next!=='\n')value+=next;}
          else value+=char;
          continue;
        }
        if(char==='"'||char==="'"){if(!quote){quote=char;continue;}if(quote===char){quote='';continue;}}
        value+=char;
      }
      if(quote)return words;
      words.push({start,end:index,value});
    }
    return words;
  }

  function executableToken(command) {
    const words=commandWords(command);let index=0;
    while(words[index]&&/^[A-Za-z_][A-Za-z0-9_]*=/.test(words[index].value))index++;
    if(words[index]?.value.split('/').pop()==='env') {
      index++;
      while(words[index]) {
        const value=words[index].value;
        if(value==='-u'||value==='--unset'){index+=2;continue;}
        if(value==='-i'||value==='--ignore-environment'||/^[A-Za-z_][A-Za-z0-9_]*=/.test(value)){index++;continue;}
        if(value.startsWith('-'))return null;
        break;
      }
    }
    const token=words[index];
    return token&&!/[|;&<>$`]/.test(token.value)&&(!/\s/.test(token.value)||token.value.includes('/'))?token:null;
  }

  const quotePath=path=>/^[A-Za-z0-9_./:+-]+$/.test(path)?path:`'${path.replace(/'/g,"'\\''")}'`;
  const familyOf=value=>{const name=value.split('/').pop();return /^python\d*(?:\.\d+)*$/.test(name)?'python':/^node(?:js)?$/.test(name)?'node':name;};

  class ExecutablePicker {
    constructor({input,icon,cwd,binding=null,notify=()=>{}}) {
      this.input=input;this.icon=icon;this.cwd=cwd;this.field=input.closest('.editor-field, .field');this.dialog=input.closest('dialog')||document.body;
      this.notify=notify;this.binding=binding?.path?{...binding,name:binding.name||binding.path.split('/').pop()}:null;
      const initial=executableToken(input.value);
      if(this.binding&&initial){input.value=input.value.slice(0,initial.start)+quotePath(this.binding.name)+input.value.slice(initial.end);}
      if(this.field.tagName==='LABEL') {
        const field=document.createElement('div'),title=this.field.querySelector(':scope > span'),label=document.createElement('label');
        field.className=this.field.className;label.htmlFor=input.id;label.innerHTML=title?.innerHTML||'실행 명령';field.append(label);
        for(const child of [...this.field.childNodes])if(child!==title)field.append(child);
        this.field.replaceWith(field);this.field=field;
      }
      this.state='idle';this.isOpen=false;this.destroyed=false;this.key='';this.result=null;
      this.wrapper=document.createElement('div');this.wrapper.className='command-input';input.before(this.wrapper);this.wrapper.append(input);
      this.trigger=document.createElement('button');this.trigger.type='button';this.trigger.className='command-runtime-toggle';
      this.trigger.setAttribute('aria-label','실행자 선택');this.trigger.setAttribute('aria-haspopup','dialog');this.trigger.setAttribute('aria-expanded','false');this.trigger.setAttribute('aria-controls','executable-picker');this.trigger.innerHTML=icon('ChevronDown');this.wrapper.append(this.trigger);
      const controls=document.createElement('div');controls.className='command-picker-controls';this.wrapper.before(controls);controls.append(this.wrapper);
      const install=document.createElement('button');install.type='button';install.className='command-install';install.setAttribute('aria-label','실행자 버전 설치');install.title='실행자 버전 설치';install.innerHTML=icon('Plus');controls.append(install);
      install.addEventListener('click',()=>{this.close();window.SupervisorRuntimeInstaller?.open({family:this.result?.family||familyOf(executableToken(this.input.value)?.value||'node'),icon:this.icon,notify:this.notify,onInstalled:entry=>{if(!this.destroyed)this.choose(quotePath(entry.path),entry);}});});
      this.summary=document.createElement('div');this.summary.className='command-runtime-summary';this.summary.setAttribute('aria-live','polite');controls.after(this.summary);
      this.panel=document.createElement('div');this.panel.id='executable-picker';this.panel.className='runtime-picker';this.panel.setAttribute('popover','auto');this.panel.setAttribute('role','dialog');this.panel.setAttribute('aria-label','실행자 선택');
      this.panel.innerHTML=`<header><strong data-runtime-title>실행자 선택</strong><span data-runtime-count></span><button type="button" class="icon-button" data-runtime-refresh aria-label="PATH 다시 검색">${icon('RefreshCw')}</button></header><label class="runtime-search">${icon('Search')}<input type="search" aria-label="실행자 검색" placeholder="버전 또는 경로로 검색" autocomplete="off"></label><div class="runtime-options" role="listbox" aria-label="사용할 실행자"></div><div class="runtime-manual" hidden><label for="runtime-path">실행 파일의 절대 경로</label><input id="runtime-path" spellcheck="false" placeholder="/opt/homebrew/bin/node"><p data-runtime-manual-error hidden></p><div><button type="button" class="button small" data-runtime-path-cancel>취소</button><button type="button" class="button primary small" data-runtime-path-apply>경로 사용</button></div></div><footer><button type="button" data-runtime-manual>경로 직접 지정…</button><span>미리보기 서버의 PATH</span></footer>`;
      this.dialog.append(this.panel);this.search=this.panel.querySelector('input[type="search"]');
      this.hasPopover=typeof this.panel.showPopover==='function';if(!this.hasPopover)this.panel.hidden=true;
      this.onInput=()=>this.schedule();this.onResize=()=>this.position();
      this.onOutside=event=>{if(this.isOpen&&!this.panel.contains(event.target)&&!this.wrapper.contains(event.target))this.close();};
      this.input.addEventListener('input',this.onInput);this.trigger.addEventListener('click',()=>this.isOpen?this.close(true):this.open());
      this.input.addEventListener('keydown',event=>{if(event.altKey&&event.key==='ArrowDown'){event.preventDefault();this.open();}});
      this.search.addEventListener('input',()=>this.renderOptions());
      this.panel.addEventListener('click',event=>this.click(event));
      this.panel.addEventListener('keydown',event=>this.keydown(event));
      this.panel.addEventListener('toggle',event=>{this.isOpen=event.newState==='open';this.trigger.setAttribute('aria-expanded',String(this.isOpen));});
      window.addEventListener('scroll',this.onResize,true);
      document.addEventListener('pointerdown',this.onOutside,true);window.addEventListener('resize',this.onResize);
      if(window.SupervisorCommandParameters)this.parameters=new window.SupervisorCommandParameters({source:input,field:this.field,summary:this.summary,icon});
      this.load();
    }

    schedule() {clearTimeout(this.timer);this.timer=setTimeout(()=>this.load(),180);}

    async load(force=false) {
      if(this.destroyed)return;
      this.token=executableToken(this.input.value);
      if(this.binding&&this.token?.value!==this.binding.name)this.binding=null;
      this.trigger.disabled=!this.token;
      if(!this.token){this.result=null;this.key='';this.summary.textContent='실행자 이름을 입력하면 설치된 버전과 경로를 확인합니다.';this.close();this.emitSelection();return;}
      const lookup=this.binding?.path||this.token.value;
      const key=`${lookup}\n${this.cwd()}`;
      if(!force&&key===this.key){this.renderSummary();return;}
      this.key=key;this.controller?.abort();this.controller=new AbortController();
      const controller=this.controller;this.state='loading';this.result=null;this.renderSummary();this.renderOptions();
      const timeout=setTimeout(()=>controller.abort(),8000);
      try {
        const parameters=new URLSearchParams({command:lookup,cwd:this.cwd()});
        const response=await fetch(`/__preview__/executables?${parameters}`,{signal:controller.signal,cache:'no-store'});
        if(!response.ok)throw new Error('discovery unavailable');
        const result=await response.json();if(!Array.isArray(result.entries))throw new Error('invalid response');
        if(this.destroyed||key!==this.key||controller!==this.controller)return;
        this.result=result;this.state='ready';
        if(this.binding)this.binding={...this.binding,...(result.current||{}),isAvailable:Boolean(result.current)};
      } catch(error) {
        if(this.destroyed||key!==this.key||controller!==this.controller)return;
        this.state='error';this.result=null;
      } finally {clearTimeout(timeout);}
      if(!this.destroyed&&key===this.key&&controller===this.controller){this.renderSummary();this.renderOptions();this.position();this.emitSelection();}
    }

    renderSummary() {
      if(this.state==='loading'){this.summary.innerHTML=`<span class="muted">${this.icon('Search')}실행자 확인 중…</span>`;return;}
      if(this.state==='error'){this.summary.innerHTML='<span class="muted">PATH 검색에 연결할 수 없습니다. 경로를 직접 지정할 수 있습니다.</span>';return;}
      const entry=this.selection();
      if(!entry){this.summary.innerHTML=`<span class="runtime-missing">${escape(this.token?.value||'')} · ${this.token?.value.includes('/')?'실행 파일을 확인할 수 없습니다.':'PATH에서 찾지 못했습니다.'}</span>`;return;}
      if(entry.isAvailable===false){this.summary.innerHTML=`<span class="runtime-missing">선택한 실행 파일을 확인할 수 없습니다.</span><code title="${escape(entry.path)}">${escape(entry.path)}</code>`;return;}
      const isAuto=!this.binding;
      this.summary.innerHTML=`<strong>${escape(this.versionLabel(entry))}</strong><span class="runtime-binding">${isAuto?'PATH 기본값':'경로 고정'}</span><code title="${escape(entry.path)}">${escape(entry.path)}</code>`;
    }

    versionLabel(entry) {return `${entry.label} ${entry.version||'· 버전 미확인'}`;}

    selection() {
      const token=executableToken(this.input.value);
      if(!token)return null;
      if(this.binding&&token.value===this.binding.name)return this.binding;
      if(!this.result||`${token.value}\n${this.cwd()}`!==this.key)return null;
      return this.result.current;
    }

    bindingSnapshot() {const token=executableToken(this.input.value);return this.binding&&token?.value===this.binding.name?{...this.binding}:null;}

    resolvedCommand() {const token=executableToken(this.input.value),binding=this.bindingSnapshot();return token&&binding?this.input.value.slice(0,token.start)+quotePath(binding.path)+this.input.value.slice(token.end):this.input.value;}

    emitSelection() {this.input.dispatchEvent(new CustomEvent('executablechange',{bubbles:true,detail:{entry:this.selection(),family:this.result?.family||familyOf(this.token?.value||''),help:this.result?.help||null}}));}

    option(entry,key,isAuto=false) {
      const isSelected=isAuto?!this.binding:this.binding?.path===entry.path;
      const title=isAuto?'PATH 기본값 사용':this.versionLabel(entry);
      const subtitle=isAuto?`${this.versionLabel(entry)} · ${entry.path}`:entry.path;
      return `<button type="button" role="option" aria-selected="${isSelected}" data-runtime-option="${key}" title="${escape(entry.resolvedPath!==entry.path?`${entry.path}\n실제 파일: ${entry.resolvedPath}`:entry.path)}"><span class="runtime-option-main"><strong>${escape(title)}</strong><code>${escape(subtitle)}</code></span><span class="runtime-option-mark">${isSelected?this.icon('Check'):''}</span></button>`;
    }

    renderOptions() {
      const list=this.panel.querySelector('.runtime-options');
      this.panel.querySelector('[data-runtime-title]').textContent=`${this.token?.value.split('/').pop()||''} 실행자`;
      this.panel.querySelector('[data-runtime-count]').textContent=this.result?`${this.result.entries.length}개 경로`:'';
      if(this.state==='loading'){list.innerHTML='<p class="runtime-empty">PATH에서 실행 파일과 버전을 확인하고 있습니다…</p>';return;}
      if(this.state==='error'){list.innerHTML='<p class="runtime-empty">로컬 PATH 검색을 사용할 수 없습니다.<br>미리보기 서버 연결을 확인하거나 경로를 직접 입력하세요.</p>';return;}
      const query=this.search.value.trim().toLowerCase(),entries=this.result?.entries||[];
      const rows=[];const auto=entries.find(entry=>entry.path===this.result?.defaultPath);
      if(auto&&(!query||`path 기본값 ${this.versionLabel(auto)} ${auto.path}`.toLowerCase().includes(query)))rows.push(this.option(auto,'auto',true));
      entries.forEach((entry,index)=>{if(!query||`${entry.name} ${entry.label} ${entry.version||''} ${entry.path}`.toLowerCase().includes(query))rows.push(this.option(entry,String(index)));});
      list.innerHTML=rows.join('')||'<p class="runtime-empty">일치하는 실행 파일이 없습니다.<br>다른 검색어를 입력하거나 경로를 직접 지정하세요.</p>';
    }

    open() {
      this.token=executableToken(this.input.value);if(!this.token)return;
      this.search.value='';this.panel.classList.remove('is-manual');this.panel.querySelector('.runtime-manual').hidden=true;
      this.renderOptions();this.isOpen=true;this.trigger.setAttribute('aria-expanded','true');
      if(this.hasPopover)this.panel.showPopover();else {this.panel.hidden=false;this.panel.dataset.fallbackOpen='true';}
      this.position();this.search.focus({preventScroll:true});this.load();
    }

    close(restoreFocus=false) {
      if(!this.isOpen)return;this.isOpen=false;
      if(this.hasPopover){try{this.panel.hidePopover();}catch{}}else {this.panel.hidden=true;delete this.panel.dataset.fallbackOpen;}
      this.trigger.setAttribute('aria-expanded','false');if(restoreFocus)this.trigger.focus({preventScroll:true});
    }

    position() {
      if(!this.isOpen)return;
      const bounds=this.wrapper.getBoundingClientRect(),width=Math.min(680,innerWidth-24);
      const below=innerHeight-bounds.bottom-14,above=bounds.top-14,useBelow=below>=240||below>=above;
      const maxHeight=Math.max(150,Math.min(410,useBelow?below:above));
      this.panel.style.width=`${width}px`;this.panel.style.maxHeight=`${maxHeight}px`;
      this.panel.style.left=`${Math.max(12,Math.min(bounds.right-width,innerWidth-width-12))}px`;
      const height=Math.min(this.panel.scrollHeight,maxHeight);
      this.panel.style.top=`${useBelow?bounds.bottom+5:Math.max(12,bounds.top-height-5)}px`;
    }

    choose(value,entry=null) {
      const token=executableToken(this.input.value)||{start:0,end:0};
      const raw=commandWords(value)[0]?.value||value,isAuto=entry&&value===entry.name;
      const name=entry?.name||raw.split('/').pop();
      this.binding=isAuto?null:entry||{name,path:raw,label:familyOf(name),family:familyOf(name),version:null};
      const display=quotePath(name);this.input.value=this.input.value.slice(0,token.start)+display+this.input.value.slice(token.end);
      if(entry){this.result={...(this.result||{}),current:entry,help:null,entries:this.result?.entries||[entry],family:entry.family,label:entry.label};this.result.defaultPath=this.result.entries.find(item=>item.name===entry.name&&item.source==='path')?.path||null;}
      this.key=`${this.binding?.path||name}\n${this.cwd()}`;this.token=executableToken(this.input.value);
      this.close();this.input.dispatchEvent(new Event('input',{bubbles:true}));this.renderSummary();this.emitSelection();
      this.load(true);
      this.focus();
    }

    click(event) {
      const button=event.target.closest('button');if(!button)return;
      if(button.hasAttribute('data-runtime-refresh'))this.load(true);
      if(button.hasAttribute('data-runtime-option')) {
        const key=button.dataset.runtimeOption,entry=key==='auto'?this.result.entries.find(item=>item.path===this.result.defaultPath):this.result.entries[Number(key)];
        if(entry)this.choose(key==='auto'?entry.name:quotePath(entry.path),entry);
      }
      if(button.hasAttribute('data-runtime-manual')){this.panel.classList.add('is-manual');this.panel.querySelector('.runtime-manual').hidden=false;this.panel.querySelector('[data-runtime-manual-error]').hidden=true;const input=this.panel.querySelector('#runtime-path');input.value=this.result?.current?.path||'';this.position();input.focus();}
      if(button.hasAttribute('data-runtime-path-cancel')){this.panel.classList.remove('is-manual');this.panel.querySelector('.runtime-manual').hidden=true;this.position();}
      if(button.hasAttribute('data-runtime-path-apply'))this.applyManualPath();
    }

    applyManualPath() {
      const value=this.panel.querySelector('#runtime-path').value.trim(),error=this.panel.querySelector('[data-runtime-manual-error]');
      if(!value.startsWith('/')||value.includes('\0')){error.textContent='실행 파일의 절대 경로를 입력하세요.';error.hidden=false;return;}
      error.hidden=true;this.choose(quotePath(value));this.load(true);
    }

    keydown(event) {
      if(event.key==='Escape'){event.preventDefault();event.stopPropagation();this.close(true);return;}
      if(event.key==='Enter'&&event.target.id==='runtime-path'){event.preventDefault();this.applyManualPath();return;}
      if(!['ArrowDown','ArrowUp','Home','End'].includes(event.key)||event.target.id==='runtime-path')return;
      const options=[...this.panel.querySelectorAll('[role="option"]')];if(!options.length)return;
      const index=options.indexOf(document.activeElement);event.preventDefault();
      const next=event.key==='Home'?0:event.key==='End'?options.length-1:event.key==='ArrowDown'?(index+1)%options.length:index<0?options.length-1:(index-1+options.length)%options.length;
      options[next].focus();
    }

    focus() {if(this.parameters)this.parameters.focus();else this.input.focus({preventScroll:true});}

    isValid() {return !this.parameters||this.parameters.isValid();}

    destroy() {
      this.destroyed=true;clearTimeout(this.timer);this.controller?.abort();this.close();this.panel.remove();this.parameters?.destroy();
      this.input.removeEventListener('input',this.onInput);window.removeEventListener('scroll',this.onResize,true);
      document.removeEventListener('pointerdown',this.onOutside,true);window.removeEventListener('resize',this.onResize);
    }
  }
  window.SupervisorCommandSyntax={words:commandWords,executable:executableToken,quote:quotePath};
  window.SupervisorExecutablePicker=ExecutablePicker;
})();
