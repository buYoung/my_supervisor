/* Node options precede the entry script; program arguments remain after it. */
(() => {
  const escape=value=>String(value??'').replace(/[&<>"']/g,char=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'})[char]);
  const quote=value=>/^[A-Za-z0-9_./:@%+,=\-]+$/.test(value)?value:`'${value.replace(/'/g,"'\\''")}'`;
  const valueOptions=new Set(['--port','-p','-jar','-cp','-classpath','--class-path','--module-path','--require','-r','--loader','--import','--inspect-port','--env-file','--config']);
  function optionHelp(help,name) {return help?.options?.[name]||(/^\-D.+/.test(name)?help?.options?.['-D']:null);}

  function automaticJoin(row,help) {
    if(!row.name)return 'none';
    if(/^\-X(?:ms|mx|ss)$/.test(row.name))return 'attached';
    if(/^\-D.+/.test(row.name))return 'equals';
    const documented=optionHelp(help,row.name)?.join;
    if(documented&&documented!=='none')return documented;
    if(row.name.startsWith('--'))return 'equals';
    return row.join!=='none'?row.join:'space';
  }

  function canStructure(command) {
    let quote='';
    for(let index=0;index<command.length;index++) {
      const char=command[index];
      if(char==='\\'&&quote!=="'") {
        if(index===command.length-1)return false;
        if(!quote||'$`"\\\n'.includes(command[index+1]))index++;
        continue;
      }
      if(char==='"'||char==="'"){if(!quote)quote=char;else if(quote===char)quote='';continue;}
      if(quote!=="'"&&'$`'.includes(char))return false;
      if(!quote&&'|;&<>()\n'.includes(char))return false;
      if(!quote&&char==='#'&&(index===0||/\s/.test(command[index-1])))return false;
    }
    return !quote;
  }

  const nodeInlineOptions=new Set(['-e','--eval','-p','--print','--run']);

  function optionRows(tokens,source,help,isNodeCommand=false,scriptHelp=null) {
    const rows=[];let isPositionalOnly=false,hasNodeEntry=false;
    for(let index=0;index<tokens.length;index++) {
      const token=tokens[index],value=token.value;
      const row={start:token.start,end:token.end,wordIndex:token.wordIndex,name:'',value,join:'none',isPositionalOnly};
      const isScriptArgument=isNodeCommand?hasNodeEntry:token.wordIndex>=scriptHelp?.argumentWordIndex;
      const rowHelp=isScriptArgument||scriptHelp?.kind==='command'?scriptHelp:help;
      if(!isPositionalOnly&&value==='--'){row.name=value;row.value='';isPositionalOnly=true;}
      else if(!isPositionalOnly&&value.startsWith('-')&&value!=='-') {
        const equals=value.indexOf('='),memory=value.match(/^(-X(?:ms|mx|ss))(.+)$/);
        if(equals>0){row.name=value.slice(0,equals);row.value=value.slice(equals+1);row.join='equals';}
        else if(memory){row.name=memory[1];row.value=memory[2];row.join='attached';}
        else {
          row.name=value;row.value='';
          const next=tokens[index+1];
          const documented=optionHelp(rowHelp,value);
          if((documented?documented.takesValue||documented.acceptsValue&&documented.join==='space':!isScriptArgument&&valueOptions.has(value))&&next&&(!next.value.startsWith('-')||/^-\d/.test(next.value))){row.value=next.value;row.join='space';row.separator=source.slice(token.end,next.start);row.end=next.end;index++;}
        }
      }
      rows.push(row);
      if(isNodeCommand&&!hasNodeEntry&&(!row.name||nodeInlineOptions.has(row.name))){hasNodeEntry=true;isPositionalOnly=false;}
    }
    return rows;
  }

  function rowSource(row) {
    if(!row.name)return quote(row.value);
    if(row.join==='none')return quote(row.name);
    const separator=row.join==='space'?(row.separator||' '):row.join==='equals'?'=':'';
    return quote(row.name)+separator+quote(row.value);
  }

  class CommandParameters {
    constructor({source,field,summary,icon}) {
      this.source=source;this.icon=icon;this.field=field;this.mode='structured';this.isWriting=false;
      this.label=field.querySelector(`label[for="${source.id}"]`);this.originalFor=this.label?.htmlFor;
      this.hint=field.querySelector(':scope > small');this.wasHintHidden=this.hint?.hidden;
      this.heading=document.createElement('div');this.heading.className='command-view-heading';
      if(this.label){this.label.before(this.heading);this.heading.append(this.label);}else field.prepend(this.heading);
      this.heading.insertAdjacentHTML('beforeend','<div class="command-view-toggle"><button type="button" data-command-mode="structured" aria-pressed="true">매개변수 편집</button><button type="button" data-command-mode="raw" aria-pressed="false">명령만</button></div>');
      this.root=document.createElement('div');this.root.className='command-parameters';
      this.root.innerHTML=`<p class="command-raw-note" hidden>쉘 연산이나 변수 확장이 있는 명령은 위 입력칸에서 편집합니다.</p><div class="command-parts"><p class="command-prefix" hidden></p><details open><summary><span>매개변수 <small data-argument-count></small></span>${icon('ChevronDown')}</summary><label class="argument-search" hidden>${icon('Search')}<input type="search" aria-label="매개변수 검색" placeholder="옵션 이름 또는 값 검색"></label><div class="argument-groups"></div><small class="argument-hint">연결 형식은 자동으로 유지합니다. 이름·값 또는 ⓘ에 마우스를 올리면 도움말을 볼 수 있습니다.</small></details></div>`;
      summary.after(this.root);
      this.onSource=()=>{if(!this.isWriting)this.read();};source.addEventListener('input',this.onSource);
      this.onHelp=event=>{this.help=event.detail.help;this.runtime=event.detail.entry;this.helpForValue=window.SupervisorCommandSyntax.executable(this.source.value)?.value;this.hasHelpResolved=true;if(this.root.contains(document.activeElement)){this.annotateRows();this.applyHints();}else this.read();this.completion?.refresh();this.scriptDiscovery?.schedule();};
      source.addEventListener('executablechange',this.onHelp);
      this.heading.addEventListener('click',event=>{const button=event.target.closest('[data-command-mode]');if(button){this.mode=button.dataset.commandMode;this.read();}});
      this.root.querySelector('details').addEventListener('toggle',()=>this.resizeArguments());
      this.root.addEventListener('input',event=>{
        const input=event.target,index=input.dataset.argumentName??input.dataset.argumentValue;
        if(index!==undefined) {
          const row=this.rows[Number(index)];
          if(input.hasAttribute('data-argument-name')) {
            const name=input.value.trim();
            row.name=name;if(row.value||row.join!=='none')row.join=automaticJoin(row,this.helpForRow(row));
          } else {row.value=input.value;if(row.name&&row.join==='none'&&row.value)row.join=automaticJoin(row,this.helpForRow(row));}
          this.hideTooltip();
          this.replaceRow(Number(index));this.updateRowControls(Number(index));this.resizeArguments();
        } else this.filter();
      });
      this.root.addEventListener('click',event=>{
        const button=event.target.closest('button');if(!button)return;
        if(button.hasAttribute('data-argument-add'))this.addRow(button.dataset.argumentAdd||'all');
        if(button.hasAttribute('data-script-help-refresh'))this.scriptDiscovery?.schedule(true);
        if(button.hasAttribute('data-argument-remove')){const index=Number(button.dataset.argumentRemove),row=this.rows[index];if(row.isDraft){this.rows.splice(index,1);this.renderRows();}else {const previous=[...this.rows.slice(0,index)].reverse().find(item=>!item.isDraft)||this.executable;this.write(this.source.value.slice(0,previous.end)+this.source.value.slice(row.end));}}
      });
      this.onResize=()=>{this.hideTooltip();this.completion?.close();this.resizeArguments();};window.addEventListener('resize',this.onResize);this.read();
      this.tooltip=document.createElement('div');this.tooltip.className='argument-tooltip';this.tooltip.id=`${source.id}-argument-tooltip`;this.tooltip.setAttribute('role','tooltip');this.tooltip.setAttribute('popover','manual');this.tooltip.hidden=true;(source.closest('dialog')||document.body).append(this.tooltip);
      if(window.SupervisorOptionCompletion)this.completion=new window.SupervisorOptionCompletion({root:this.root,parent:source.closest('dialog')||document.body,id:source.id,getHelp:index=>this.helpForRow(this.rows[index]),getRows:()=>this.rows,onOpen:()=>this.hideTooltip(),onSelect:(index,name,details)=>this.selectOption(index,name,details)});
      this.root.addEventListener('pointerover',event=>{const target=event.target.closest('[data-argument-help]');if(target)this.showTooltip(target);});
      this.root.addEventListener('pointerout',event=>{if(event.target.closest('[data-argument-help]')&&!event.relatedTarget?.closest?.('[data-argument-help]'))this.hideTooltip();});
      this.root.addEventListener('focusin',event=>{if(event.target.matches('[data-argument-help]'))this.showTooltip(event.target);});
      this.root.addEventListener('focusout',()=>this.hideTooltip());
      this.onEscape=event=>{if(event.key==='Escape'&&(this.completion?.isOpen||this.tooltip&&!this.tooltip.hidden)){event.preventDefault();event.stopPropagation();this.isDismissingTooltip=true;this.hideTooltip();this.completion?.close();clearTimeout(this.escapeTimer);this.escapeTimer=setTimeout(()=>{this.isDismissingTooltip=false;},100);}};
      this.onCancel=event=>{if(this.isDismissingTooltip||this.completion?.isOpen||this.tooltip&&!this.tooltip.hidden){event.preventDefault();this.hideTooltip();this.completion?.close();}};
      document.addEventListener('keydown',this.onEscape,true);this.parentDialog=source.closest('dialog');this.parentDialog?.addEventListener('cancel',this.onCancel);
      this.onScroll=event=>{this.hideTooltip();if(!this.completion?.contains(event.target))this.completion?.position();};window.addEventListener('scroll',this.onScroll,true);
      if(window.SupervisorScriptHelp)this.scriptDiscovery=new window.SupervisorScriptHelp({source,getRequest:previous=>this.helpRequest(previous),onChange:result=>this.updateScriptHelp(result)});
    }

    updateScriptHelp(result) {
      const active=document.activeElement,index=active?.dataset.argumentName??active?.dataset.argumentValue;
      const focusedRow=this.rows[Number(index)],start=focusedRow?.start,isValue=active?.hasAttribute('data-argument-value');
      const selection=[active?.selectionStart,active?.selectionEnd];
      this.scriptHelp=result;
      if(result.status!=='loading'&&!this.rows.some(row=>row.isDraft)) {
        this.read();
        if(focusedRow&&this.root.contains(active)===false) {
          const nextIndex=this.rows.findIndex(row=>row.start===start||row.start<start&&row.end>=start);
          const next=this.root.querySelector(`[data-argument-${isValue?'value':'name'}="${nextIndex}"]`);
          if(next&&!next.disabled){next.focus({preventScroll:true});if(selection[0]!=null)next.setSelectionRange(...selection);}
        }
      } else {this.applyHints();this.renderHelpStatus();}
      this.completion?.refresh();
    }

    helpRequest(previous) {
      const syntax=window.SupervisorCommandSyntax,executable=syntax.executable(this.source.value);
      if(!executable||!canStructure(this.source.value))return null;
      const words=syntax.words(this.source.value),tokens=words.filter(token=>token.start>=executable.start),argv=tokens.map(token=>token.value);
      if(this.runtime?.path&&this.helpForValue===executable.value)argv[0]=this.runtime.path;
      const parsed=window.SupervisorEnvironment?.parse(this.source.form?.elements.environment?.value||'');
      if(parsed?.error)return null;
      const environment={...(parsed?.environment||{})};
      for(const token of words.filter(token=>token.end<=executable.start)){const match=token.value.match(/^([A-Za-z_]\w*)=(.*)$/s);if(match)environment[match[1]]=match[2];}
      const cwd=this.source.form?.elements.cwd?.value||'';
      const entry=this.rows.find(row=>row.scope==='entry'&&!row.isDraft);
      const boundary=entry?entry.wordIndex+1:previous?.argumentWordIndex||argv.length;
      const leading=[];for(const value of argv.slice(boundary)){if(value.startsWith('-'))break;leading.push(value);}
      return {argv,cwd,environment,key:JSON.stringify([argv.slice(0,boundary),leading,cwd,environment])};
    }

    helpForRow(row) {return row?.isProgramArgument||this.scriptHelp?.kind==='command'?this.scriptHelp:this.help;}

    renderHelpStatus() {
      const status=this.root.querySelector('[data-script-help-status]');if(!status)return;
      const help=this.scriptHelp,count=new Set(Object.values(help?.options||{}).map(option=>option.syntax)).size;
      status.dataset.state=help?.status||'idle';
      status.querySelector('span').textContent=help?.status==='ready'?`${help.label} · ${count}개 옵션 · ${help.cacheHit?'캐시 사용':'도움말 캐시됨'}`:help?.reason||'실행할 파일이나 CLI 명령을 지정하면 도움말을 읽습니다.';
      status.title=help?.status==='ready'?`${help.sourceCommand}\n${help.path}\n${help.cachedAt}`:'';
      status.querySelector('button').disabled=help?.status==='loading'||!this.executable;
    }

    read() {
      this.completion?.close();
      const syntax=window.SupervisorCommandSyntax;
      const previousExecutable=this.executable?.value;
      this.executable=syntax.executable(this.source.value);
      this.isNodeCommand=/^(?:.*\/)?node(?:js)?$/.test(this.executable?.value||'');
      if(this.executable?.value!==this.helpForValue){this.help=null;this.hasHelpResolved=false;}
      if(this.executable?.value!==previousExecutable)this.scriptHelp=null;
      const tokens=this.executable?syntax.words(this.source.value).filter(token=>token.start>=this.executable.end).map((token,index)=>({...token,wordIndex:index+1})):[];
      this.rows=optionRows(tokens,this.source.value,this.help,this.isNodeCommand,this.scriptHelp);this.annotateRows();
      const isStructurable=canStructure(this.source.value)&&(!this.source.value.trim()||Boolean(this.executable));
      const isStructured=this.mode==='structured'&&isStructurable;
      this.source.hidden=false;this.source.title=this.source.value;
      if(this.label)this.label.htmlFor=this.originalFor;
      if(this.hint)this.hint.hidden=isStructured||this.wasHintHidden;
      this.heading.querySelector('[data-command-mode="structured"]').disabled=!isStructurable;
      this.heading.querySelectorAll('[data-command-mode]').forEach(button=>button.setAttribute('aria-pressed',String(button.dataset.commandMode===(isStructured?'structured':'raw'))));
      this.root.querySelector('.command-raw-note').hidden=isStructurable;
      this.root.querySelector('.command-parts').hidden=!isStructured;
      const prefix=this.source.value.slice(0,this.executable?.start||0).trim(),prefixElement=this.root.querySelector('.command-prefix');
      prefixElement.textContent=prefix?`접두사 · ${prefix}`:'';prefixElement.hidden=!prefix;
      this.renderRows();
    }

    renderRows() {
      const groups=this.root.querySelector('.argument-groups');
      const renderList=(rows,empty)=>rows.length?`<div class="argument-list"><div class="argument-columns" aria-hidden="true"><span></span><span>옵션 / 역할</span><span>값</span><span></span><span></span></div><div class="argument-rows">${rows.map(row=>this.renderRow(row,this.rows.indexOf(row))).join('')}</div></div>`:`<p class="argument-empty">${empty}</p>`;
      if(this.isNodeCommand) {
        const node=this.rows.filter(row=>row.scope==='node'),entry=this.rows.filter(row=>row.scope==='entry'),script=this.rows.filter(row=>row.scope==='script');
        const isInline=this.rows.some(row=>row.scope==='node'&&nodeInlineOptions.has(row.name));
        groups.innerHTML=`<section class="argument-group" data-argument-group="node"><header><strong>Node 옵션 <small>${node.length}개</small></strong><span>실행 스크립트 앞</span></header>${renderList(node,'추가한 Node 옵션이 없습니다.')}<button type="button" class="argument-add" data-argument-add="node">${this.icon('Plus')}Node 옵션 추가</button></section><section class="argument-group argument-entry" data-argument-group="entry"><header><strong>실행 스크립트</strong><span>${isInline?'인라인 실행 사용':'JavaScript 진입 파일'}</span></header>${renderList(entry,isInline?'실행 코드는 Node 옵션에서 관리합니다.':'실행할 스크립트를 지정하세요.')}${!entry.length&&!isInline?`<button type="button" class="argument-add" data-argument-add="entry">${this.icon('Plus')}스크립트 지정</button>`:''}</section><section class="argument-group" data-argument-group="script"><header><strong>스크립트 인수 <small>${script.length}개</small></strong><span>실행 스크립트 뒤</span></header>${renderList(script,'스크립트에 전달할 인수가 없습니다.')}<button type="button" class="argument-add" data-argument-add="script" ${!entry.length&&!isInline?'disabled':''}>${this.icon('Plus')}스크립트 인수 추가</button></section>`;
      } else groups.innerHTML=`${renderList(this.rows,'전달할 매개변수가 없습니다.')}<button type="button" class="argument-add" data-argument-add="all">${this.icon('Plus')}매개변수 추가</button>`;
      const helpStatus=document.createElement('div');helpStatus.className='script-help-status';helpStatus.dataset.scriptHelpStatus='';helpStatus.setAttribute('role','status');helpStatus.innerHTML=`<span></span><button type="button" data-script-help-refresh aria-label="스크립트 도움말 다시 읽기">${this.icon('RefreshCw')}다시 읽기</button>`;
      const scriptHeader=groups.querySelector('[data-argument-group="script"] > header');if(scriptHeader)scriptHeader.after(helpStatus);else groups.prepend(helpStatus);this.renderHelpStatus();
      this.root.querySelector('.argument-hint').textContent=this.isNodeCommand?'Node 옵션은 스크립트 앞, 스크립트 인수는 뒤에 반영됩니다.':'연결 형식은 자동으로 유지합니다. 이름·값 또는 ⓘ에 마우스를 올리면 도움말을 볼 수 있습니다.';
      this.root.querySelector('.argument-search').hidden=this.rows.length<7;
      if(this.rows.length<7)this.root.querySelector('input[type="search"]').value='';
      this.root.querySelectorAll('[data-argument-add]').forEach(button=>{if(!this.executable)button.disabled=true;});
      this.filter();this.applyHints();
    }

    addRow(scope) {
      this.root.querySelector('input[type="search"]').value='';
      if(!this.isNodeCommand){this.write(`${this.source.value} ''`);const index=this.rows.length-1;this.root.querySelector(`[data-argument-name="${index}"]`).focus();return;}
      const entryIndex=this.rows.findIndex(row=>row.scope==='entry'),delimiterIndex=this.rows.findIndex(row=>row.scope==='node'&&row.name==='--');
      const boundaryIndex=delimiterIndex>=0?delimiterIndex:entryIndex;
      const position=scope==='node'&&boundaryIndex>=0?boundaryIndex:this.rows.length;
      const start=position<this.rows.length?this.rows[position].start:this.source.value.length;
      const row={start,end:start,name:'',value:'',join:'none',scope,isDraft:true,isProgramArgument:scope==='script',isPositionalOnly:false};
      if(scope==='entry')row.role={kind:'script',label:'실행 스크립트',description:'Node.js가 실행할 JavaScript 진입 파일입니다.',usage:this.help?.usage};
      this.rows.splice(position,0,row);this.renderRows();
      this.root.querySelector(scope==='entry'?`[data-argument-value="${position}"]`:`[data-argument-name="${position}"]`).focus();
    }

    renderRow(row,index) {
      return `<div class="argument-row" data-argument-row="${index}"><span class="argument-number">${String(index+1).padStart(2,'0')}</span><div class="argument-name-field"><input class="argument-name" data-argument-name="${index}" data-argument-help="${index}" role="combobox" aria-autocomplete="list" aria-expanded="false" aria-controls="${this.source.id}-option-list" aria-label="매개변수 ${index+1} 옵션 이름" placeholder="이름 없음" autocomplete="off" spellcheck="false" value="${escape(row.name)}"><button type="button" class="argument-option-toggle" data-option-open="${index}" aria-label="매개변수 ${index+1} 옵션 검색">${this.icon('ChevronDown')}</button></div><textarea rows="1" data-argument-value="${index}" data-argument-help="${index}" aria-label="매개변수 ${index+1} 값" placeholder="값" spellcheck="false">${escape(row.value)}</textarea><button type="button" class="icon-button argument-help" data-argument-help="${index}" aria-label="매개변수 ${index+1} 도움말">${this.icon('Info')||'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7v1"/></svg>'}</button><button type="button" class="icon-button" data-argument-remove="${index}" aria-label="매개변수 ${index+1} 삭제">${this.icon('X')}</button></div>`;
    }

    updateRowControls(index) {
      const row=this.rows[index],element=this.root.querySelector(`[data-argument-row="${index}"]`),input=element.querySelector('textarea'),name=element.querySelector('input');
      const help=this.helpForRow(row),documented=optionHelp(help,row.name);
      input.disabled=row.name==='--'||Boolean(documented&&!documented.acceptsValue&&row.join==='none');input.placeholder=input.disabled?'값 없는 옵션':'값';
      const isRoleLabel=!row.name&&Boolean(row.role);
      name.placeholder=isRoleLabel?row.role.label:(row.isProgramArgument?'스크립트 인수':'이름 없음');name.readOnly=isRoleLabel;name.disabled=row.isPositionalOnly&&!row.role;name.dataset.role=String(isRoleLabel);
      name.setAttribute('role',isRoleLabel?'textbox':'combobox');
      if(isRoleLabel){name.removeAttribute('aria-autocomplete');name.removeAttribute('aria-expanded');name.removeAttribute('aria-controls');}
      else {name.setAttribute('aria-autocomplete','list');name.setAttribute('aria-controls',`${this.source.id}-option-list`);if(!name.hasAttribute('aria-expanded'))name.setAttribute('aria-expanded','false');}
      const toggle=element.querySelector('[data-option-open]');toggle.hidden=isRoleLabel||name.disabled;toggle.disabled=!Object.keys(help?.options||{}).length;
      const helpButton=element.querySelector('.argument-help');helpButton.disabled=!row.name&&!row.value;helpButton.style.visibility=helpButton.disabled?'hidden':'';
    }

    selectOption(index,name,details) {
      const row=this.rows[index];if(!row)return;
      row.name=name;
      if(!details.acceptsValue){row.value='';row.join='none';}
      else row.join=details.takesValue||row.value?automaticJoin(row,this.helpForRow(row)):'none';
      const entry=this.rows.find(item=>item.scope==='entry'&&!item.isDraft);
      const boundary=this.rows.find(item=>item.scope==='node'&&item.name==='--'&&!item.isDraft)||entry;
      let selectedStart=row.start;
      if(this.isNodeCommand&&!row.isProgramArgument&&boundary&&row.start>boundary.start) {
        const previous=[...this.rows.slice(0,index)].reverse().find(item=>!item.isDraft)||this.executable;
        const without=row.isDraft?this.source.value:this.source.value.slice(0,previous.end)+this.source.value.slice(row.end);
        selectedStart=boundary.start;
        this.write(without.slice(0,boundary.start)+rowSource(row)+' '+without.slice(boundary.start));
      } else {this.replaceRow(index);selectedStart=row.start;this.read();}
      const selectedIndex=this.rows.findIndex(item=>item.start===selectedStart);
      const target=this.root.querySelector(details.acceptsValue?`[data-argument-value="${selectedIndex}"]`:`[data-argument-name="${selectedIndex}"]`);
      target?.focus({preventScroll:true});target?.select();
    }

    annotateRows() {
      let hasEntry=false;const positional=this.help?.positional||(this.isNodeCommand?{kind:'script',label:'실행 스크립트',description:'Node.js가 실행할 JavaScript 진입 파일입니다.',usage:'',inlineOptions:[...nodeInlineOptions]}:null);
      for(const row of this.rows){if(row.isDraft)continue;delete row.role;row.isProgramArgument=false;row.scope=hasEntry?'script':'node';
        if(this.scriptHelp?.kind==='command'){row.scope='script';row.isProgramArgument=true;continue;}
        if(!this.isNodeCommand&&this.scriptHelp?.argumentWordIndex&&row.wordIndex>=this.scriptHelp.argumentWordIndex){row.scope='script';row.isProgramArgument=true;continue;}
        if(!this.isNodeCommand&&this.scriptHelp?.entryWordIndex===row.wordIndex&&!row.name){row.role={kind:'script',label:'실행 스크립트',description:'실행자에 전달되는 진입 파일입니다.',usage:this.help?.usage};row.scope='entry';hasEntry=true;continue;}
        if(hasEntry&&positional){row.isProgramArgument=true;continue;}
        if(row.name==='--')continue;
        if(this.help?.family==='java'&&row.name==='-jar'&&row.value){row.role={label:'실행 JAR',description:'Java가 실행할 JAR 파일입니다.',usage:this.help.usage};hasEntry=true;continue;}
        if(positional?.inlineOptions?.includes(row.name)){hasEntry=true;continue;}
        if(!row.name&&positional){row.role=positional;row.scope='entry';hasEntry=true;}
      }
    }

    applyHints() {for(let index=0;index<this.rows.length;index++)this.updateRowControls(index);}

    rowHelp(row) {
      const help=this.helpForRow(row),documented=optionHelp(help,row.name);
      const source=help?.status==='ready'?`${help.label} ${help.version||''} · ${documented?.sourceCommand||'--help'} · 도움말 캐시`:'';
      if(row.role)return {title:row.role.label,description:row.role.description,syntax:row.role.usage,source};
      if(documented){const extra=[documented.defaultValue!=null?`기본값: ${documented.defaultValue}`:'',documented.choices?`선택값: ${documented.choices}`:'',documented.variadic?'여러 값 입력 가능':''].filter(Boolean).join(' · ');return {title:row.name,description:[documented.description||'도움말에 정의된 옵션입니다.',extra].filter(Boolean).join('\n'),syntax:documented.syntax,source};}
      if(row.isProgramArgument)return {title:row.name||'스크립트 인수',description:help?.status==='loading'?'스크립트 도움말을 읽고 있습니다. 직접 입력할 수 있습니다.':help?.status==='ready'?'스크립트 도움말에 없는 인수입니다. 직접 입력한 값을 그대로 전달합니다.':'스크립트 도움말을 사용할 수 없습니다. 직접 입력한 값을 그대로 전달합니다.',syntax:help?.usage,source};
      return {title:row.name||'위치 인수',description:!this.hasHelpResolved?'실행자 도움말을 확인하고 있습니다.':this.help?.status==='ready'?'실행자 도움말에서 이 항목의 설명을 찾지 못했습니다.':'선택한 실행자의 도움말을 읽을 수 없습니다. 이름과 값은 직접 편집할 수 있습니다.',syntax:'',source};
    }

    showTooltip(anchor) {
      const row=this.rows[Number(anchor.dataset.argumentHelp)];if(!row||!this.tooltip)return;
      if(this.completion?.isOpen||this.completion?.isApplying||anchor.hasAttribute('data-argument-name')&&!row.name.trim()||anchor.hasAttribute('data-argument-value')&&anchor.disabled){this.hideTooltip();return;}
      const help=this.rowHelp(row);this.tooltip.innerHTML=`<strong>${escape(help.title)}</strong><p>${escape(help.description)}</p>${help.syntax?`<code>${escape(help.syntax)}</code>`:''}${help.source?`<small>${escape(help.source)}</small>`:''}`;
      this.tooltipAnchor?.removeAttribute('aria-describedby');this.tooltipAnchor=anchor;anchor.setAttribute('aria-describedby',this.tooltip.id);this.tooltip.hidden=false;
      if(this.tooltip.showPopover&&!this.tooltip.matches(':popover-open'))this.tooltip.showPopover();
      const bounds=anchor.getBoundingClientRect(),size=this.tooltip.getBoundingClientRect();
      this.tooltip.style.left=`${Math.max(12,Math.min(bounds.left,innerWidth-size.width-12))}px`;this.tooltip.style.top=`${bounds.bottom+size.height+10<innerHeight?bounds.bottom+7:Math.max(12,bounds.top-size.height-7)}px`;
    }

    hideTooltip() {this.tooltipAnchor?.removeAttribute('aria-describedby');this.tooltipAnchor=null;if(this.tooltip?.matches(':popover-open'))this.tooltip.hidePopover();if(this.tooltip)this.tooltip.hidden=true;}

    replaceRow(index) {
      const row=this.rows[index],raw=rowSource(row),start=row.start,end=row.end;
      const before=row.isDraft&&start>0&&!/\s/.test(this.source.value[start-1])?' ':'';
      const after=row.isDraft&&start<this.source.value.length&&!/\s/.test(this.source.value[start])?' ':'';
      const content=before+raw+after,difference=content.length-(end-start);
      this.source.value=this.source.value.slice(0,start)+content+this.source.value.slice(end);row.start=start+before.length;row.end=row.start+raw.length;delete row.isDraft;
      for(let next=index+1;next<this.rows.length;next++){this.rows[next].start+=difference;this.rows[next].end+=difference;}
      this.emitChange();
    }

    emitChange() {this.source.title=this.source.value;this.isWriting=true;this.source.dispatchEvent(new Event('input',{bubbles:true}));this.isWriting=false;}
    write(command) {this.source.value=command;this.emitChange();this.read();}

    filter() {
      const query=this.root.querySelector('input[type="search"]').value.toLowerCase();let visible=0;
      this.root.querySelectorAll('[data-argument-row]').forEach(element=>{const row=this.rows[Number(element.dataset.argumentRow)];element.hidden=!`${row.name} ${row.value}`.toLowerCase().includes(query);if(!element.hidden)visible++;});
      this.root.querySelector('[data-argument-count]').textContent=query?`${visible} / ${this.rows.length}개`:`${this.rows.length}개`;
      this.resizeArguments();
    }

    resizeArguments() {
      cancelAnimationFrame(this.frame);this.frame=requestAnimationFrame(()=>this.root.querySelectorAll('.argument-row:not([hidden]) textarea').forEach(input=>{input.style.height='0px';const height=Math.max(32,input.scrollHeight+2);input.style.height=`${Math.min(120,height)}px`;input.style.overflowY=height>120?'auto':'hidden';}));
    }

    isValid() {return Boolean(this.source.value.trim());}
    focus() {this.source.focus({preventScroll:true});this.source.select();}

    destroy() {
      cancelAnimationFrame(this.frame);clearTimeout(this.escapeTimer);document.removeEventListener('keydown',this.onEscape,true);this.parentDialog?.removeEventListener('cancel',this.onCancel);window.removeEventListener('resize',this.onResize);window.removeEventListener('scroll',this.onScroll,true);this.source.removeEventListener('input',this.onSource);this.source.removeEventListener('executablechange',this.onHelp);this.scriptDiscovery?.destroy();this.completion?.destroy();this.hideTooltip();this.tooltip?.remove();this.root.remove();
      if(this.hint)this.hint.hidden=this.wasHintHidden;
      if(this.label){this.label.htmlFor=this.originalFor;this.heading.before(this.label);}this.heading.remove();
    }
  }
  window.SupervisorCommandParameters=CommandParameters;
})();
