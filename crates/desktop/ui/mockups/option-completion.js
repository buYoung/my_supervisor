/* Search the selected runtime's cached help without restricting manual option names. */
(() => {
  const escape=value=>String(value??'').replace(/[&<>"']/g,char=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'})[char]);

  function catalogFromHelp(help) {
    const groups=new Map();
    for(const [name,details] of Object.entries(help?.options||{})) {
      const key=details.syntax+'\n'+details.description;
      if(!groups.has(key))groups.set(key,{...details,names:[]});
      groups.get(key).names.push(name);
    }
    return [...groups.values()].map(item=>({...item,name:item.names.find(name=>name.startsWith('--'))||item.names[0]}));
  }

  class OptionCompletion {
    constructor({root,parent,id,getHelp,getRows,onOpen,onSelect}) {
      this.root=root;this.getHelp=getHelp;this.getRows=getRows;this.onOpen=onOpen;this.onSelect=onSelect;this.isOpen=false;this.activeIndex=-1;
      this.panel=document.createElement('div');this.panel.className='option-completion';this.panel.setAttribute('popover','manual');this.panel.hidden=true;
      this.listId=`${id}-option-list`;
      this.panel.innerHTML=`<header><strong>실행자 옵션</strong><span data-completion-count></span></header><div class="option-completion-list" id="${this.listId}" role="listbox" aria-label="도움말 옵션 검색 결과"></div><footer><span data-completion-context>직접 입력 가능</span><span>↑↓ 이동 · Enter 선택 · Esc 닫기</span></footer>`;
      parent.append(this.panel);
      root.addEventListener('focusin',event=>{if(this.isNameInput(event.target)&&!this.isApplying)this.open(event.target);else if(!this.isApplying)this.close();});
      root.addEventListener('input',event=>{if(this.isNameInput(event.target)){this.isShowingAll=false;this.open(event.target);}else this.close();});
      root.addEventListener('click',event=>{const button=event.target.closest('[data-option-open]');if(button){const input=root.querySelector(`[data-argument-name="${button.dataset.optionOpen}"]`);input.focus({preventScroll:true});this.open(input,true);}});
      root.addEventListener('keydown',event=>this.keydown(event));
      this.panel.addEventListener('pointerdown',event=>{if(event.target.closest('[data-option-choice]'))event.preventDefault();});
      this.panel.addEventListener('click',event=>{const option=event.target.closest('[data-option-choice]');if(option)this.choose(Number(option.dataset.optionChoice));});
      this.panel.addEventListener('pointermove',event=>{const option=event.target.closest('[data-option-choice]');if(option)this.setActive(Number(option.dataset.optionChoice));});
      this.onOutside=event=>{if(this.isOpen&&!this.panel.contains(event.target)&&!event.target.closest?.('.argument-name-field'))this.close();};
      document.addEventListener('pointerdown',this.onOutside,true);
    }

    isNameInput(input) {return input?.matches?.('[data-argument-name]')&&!input.readOnly&&!input.disabled;}
    contains(target) {return target instanceof Node&&this.panel.contains(target);}

    helpForInput(input=this.input) {return this.getHelp(Number(input?.dataset.argumentName));}

    refresh() {
      const help=this.helpForInput(),key=help?.key||'';
      if(key!==this.catalogKey){this.catalogKey=key;this.catalog=catalogFromHelp(help);}
      if(this.isOpen&&this.input?.isConnected){if(this.catalog?.length)this.render();else this.close();}
      else if(this.isNameInput(document.activeElement)&&this.root.contains(document.activeElement))this.open(document.activeElement);
    }

    open(input,shouldShowAll=false) {
      if(!this.isNameInput(input)||!Object.keys(this.helpForInput(input)?.options||{}).length){this.close();return;}
      if(this.input!==input)this.close();
      this.input=input;this.isShowingAll=shouldShowAll;this.onOpen();
      const help=this.helpForInput(),key=help?.key||'';
      if(key!==this.catalogKey){this.catalogKey=key;this.catalog=catalogFromHelp(help);}
      this.isOpen=true;this.panel.hidden=false;input.setAttribute('aria-expanded','true');
      this.render();if(this.panel.showPopover&&!this.panel.matches(':popover-open'))this.panel.showPopover();this.position();
    }

    render() {
      if(!this.input?.isConnected){this.close();return;}
      const help=this.helpForInput(),query=this.isShowingAll?'':this.input.value.trim().toLowerCase();
      const terms=query.split(/\s+/).filter(Boolean);
      this.choices=(this.catalog||[]).filter(item=>terms.every(term=>`${item.names.join(' ')} ${item.description}`.toLowerCase().includes(term)));
      this.choices.sort((left,right)=>{
        const rank=item=>query&&item.names.some(name=>name.toLowerCase().startsWith(query))?0:1;
        return rank(left)-rank(right)||left.name.localeCompare(right.name);
      });
      this.activeIndex=-1;this.input.removeAttribute('aria-activedescendant');
      this.panel.querySelector('header strong').textContent=`${help.label||'실행자'} ${help.version||''} 옵션`;
      this.panel.querySelector('[data-completion-count]').textContent=`${this.choices.length}개`;
      this.panel.querySelector('.option-completion-list').innerHTML=this.choices.map((item,index)=>{
        const label=item.takesValue?'값 필수':item.acceptsValue?'값 선택':'값 없음';
        return `<button type="button" tabindex="-1" role="option" id="${this.listId}-${index}" aria-selected="false" data-option-choice="${index}"><span class="completion-option-content"><span class="completion-option-name"><strong>${escape(item.name)}</strong>${item.names.length>1?`<small>${escape(item.names.filter(name=>name!==item.name).join(', '))}</small>`:''}</span><span class="completion-option-description">${escape(item.description||item.syntax)}</span></span><span class="completion-value-kind">${label}</span></button>`;
      }).join('')||'<p class="completion-empty">일치하는 옵션이 없습니다. 입력한 이름을 그대로 사용할 수 있습니다.</p>';
      const row=this.getRows()[Number(this.input.dataset.argumentName)];
      this.panel.querySelector('[data-completion-context]').textContent=help.kind==='script'?'스크립트 뒤에 추가 · 직접 입력 가능':row?.scope==='node'&&help.family==='node'?'스크립트 앞에 추가 · 직접 입력 가능':'직접 입력 가능';
      this.position();
    }

    position() {
      if(!this.isOpen||!this.input?.isConnected)return;
      const bounds=this.input.getBoundingClientRect(),width=Math.min(610,innerWidth-24),below=innerHeight-bounds.bottom-12,above=bounds.top-12;
      if(bounds.bottom<0||bounds.top>innerHeight){this.close();return;}
      const isBelow=below>=210||below>=above,maxHeight=Math.max(150,Math.min(380,isBelow?below:above));
      this.panel.style.width=`${width}px`;this.panel.style.maxHeight=`${maxHeight}px`;
      this.panel.style.left=`${Math.max(12,Math.min(bounds.left,innerWidth-width-12))}px`;
      this.panel.style.top=`${isBelow?bounds.bottom+5:Math.max(12,bounds.top-Math.min(this.panel.scrollHeight,maxHeight)-5)}px`;
    }

    setActive(index,shouldScroll=false) {
      this.activeIndex=index;
      this.panel.querySelectorAll('[data-option-choice]').forEach((option,itemIndex)=>option.setAttribute('aria-selected',String(itemIndex===index)));
      const option=this.panel.querySelector(`[data-option-choice="${index}"]`);
      if(option){this.input.setAttribute('aria-activedescendant',option.id);if(shouldScroll)option.scrollIntoView({block:'nearest'});}
    }

    keydown(event) {
      if(!this.isNameInput(event.target))return;
      if(['ArrowDown','ArrowUp'].includes(event.key)) {
        event.preventDefault();if(!this.isOpen)this.open(event.target,event.altKey);if(!this.choices?.length)return;
        const next=event.key==='ArrowDown'?(this.activeIndex+1)%this.choices.length:this.activeIndex<0?this.choices.length-1:(this.activeIndex-1+this.choices.length)%this.choices.length;
        this.setActive(next,true);
      } else if(event.key==='Enter'&&this.isOpen){event.preventDefault();if(this.activeIndex>=0)this.choose(this.activeIndex);else this.close();}
      else if(event.key==='Tab')this.close();
    }

    choose(index) {
      const item=this.choices[index];if(!item||!this.input)return;
      const rowIndex=Number(this.input.dataset.argumentName),query=this.input.value.trim();
      const name=item.names.includes(query)?query:item.name;
      this.isApplying=true;this.close();try{this.onSelect(rowIndex,name,item);}finally{this.isApplying=false;}
    }

    close() {
      this.input?.setAttribute('aria-expanded','false');this.input?.removeAttribute('aria-activedescendant');this.input=null;this.isOpen=false;
      if(this.panel.matches(':popover-open'))this.panel.hidePopover();this.panel.hidden=true;
    }

    destroy() {this.close();document.removeEventListener('pointerdown',this.onOutside,true);this.panel.remove();}
  }
  window.SupervisorOptionCompletion=OptionCompletion;
})();
