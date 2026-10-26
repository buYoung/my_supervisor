/* A labelled sample folder browser for the standalone HTML design, not Finder. */
(() => {
  const escape=value=>String(value).replace(/[&<>"']/g,char=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'})[char]);
  const folders={
    '/':['Users','Volumes'], '/Users':['demo'], '/Users/demo':['services','projects','Downloads'],
    '/Users/demo/services':['api','api-next','worker','backup'],
    '/Users/demo/services/api':['config','dist','logs','src','package.json','README.md'],
    '/Users/demo/services/api/config':['production.json','staging.json'],
    '/Users/demo/services/api/dist':['server.js'], '/Users/demo/services/api/src':['server.ts','routes'],
    '/Users/demo/services/api-next':['config','dist','src'], '/Users/demo/services/worker':['dist','config'],
    '/Users/demo/services/backup':['backup.toml','logs'], '/Users/demo/projects':['my-app'],
    '/Volumes':['Backup'], '/Volumes/Backup':['daily','monthly']
  };
  class SampleFolderDialog {
    constructor({path,icon,onChoose}) {
      this.path=path || '/Users/demo/services';this.icon=icon;this.onChoose=onChoose;
      this.dialog=document.createElement('dialog');this.dialog.className='sample-folder-dialog';this.dialog.setAttribute('aria-labelledby','folder-dialog-title');
      this.dialog.innerHTML=`<header class="folder-header"><div><span>HTML 목업 · 샘플 폴더</span><h2 id="folder-dialog-title">${onChoose?'작업 폴더 선택':'작업 폴더 열기'}</h2></div><button class="icon-button" type="button" data-folder-close aria-label="폴더 창 닫기">${icon('X')}</button></header><form class="folder-location"><button class="button small" type="button" data-folder-up aria-label="상위 폴더">${icon('ArrowUp')}</button><label><span class="sr-only">폴더 경로</span><input name="folderPath" class="mono" spellcheck="false" value="${escape(this.path)}"></label><button class="button small" type="submit">이동</button></form><div class="folder-items"></div><footer class="folder-footer"><span class="mono" data-folder-path></span><div><button class="button" type="button" data-folder-close>${onChoose?'취소':'닫기'}</button>${onChoose?'<button class="button primary" type="button" data-folder-use>이 폴더 사용</button>':''}</div></footer>`;
      document.body.append(this.dialog);
      this.dialog.addEventListener('click',event=>{const button=event.target.closest('button');if(!button)return;if(button.hasAttribute('data-folder-close'))this.dialog.close();if(button.hasAttribute('data-folder-up')){this.path=this.path.split('/').slice(0,-1).join('/') || '/';this.render();}if(button.dataset.folderName){this.path=`${this.path==='/'?'':this.path}/${button.dataset.folderName}`;this.render();}if(button.hasAttribute('data-folder-use')){const path=this.path;this.dialog.close();this.onChoose(path);}});
      this.dialog.querySelector('form').addEventListener('submit',event=>{event.preventDefault();const input=event.currentTarget.elements.folderPath;if(!input.value.startsWith('/')){input.setCustomValidity('절대 경로를 입력하세요.');input.reportValidity();return;}input.setCustomValidity('');this.path=input.value.replace(/\/+$/,'') || '/';this.render();});
      this.dialog.addEventListener('close',()=>this.dialog.remove(),{once:true});this.render();this.dialog.showModal();
    }
    render() {
      this.dialog.querySelector('input').value=this.path;
      this.dialog.querySelector('[data-folder-path]').textContent=this.path;this.dialog.querySelector('[data-folder-path]').title=this.path;
      this.dialog.querySelector('[data-folder-up]').disabled=this.path==='/';
      const entries=folders[this.path] || [];
      this.dialog.querySelector('.folder-items').innerHTML=entries.length?entries.map(name=>{const isFile=name.includes('.');return isFile?`<div class="folder-file">${this.icon('FileText')}<span>${escape(name)}</span><small>파일</small></div>`:`<button type="button" data-folder-name="${escape(name)}">${this.icon('Folder')}<span>${escape(name)}</span>${this.icon('ChevronRight')}</button>`;}).join(''):'<div class="empty"><p>이 경로의 샘플 항목이 없습니다.</p></div>';
    }
  }
  window.SampleFolderDialog=SampleFolderDialog;

  class InstallFolderDialog {
    constructor({path,folderName,icon,onChoose}) {
      this.icon=icon;this.onChoose=onChoose;this.path=path;this.folderName=folderName;
      this.dialog=document.createElement('dialog');this.dialog.className='sample-folder-dialog install-folder-dialog';this.dialog.setAttribute('aria-label','설치 폴더 선택');
      this.dialog.innerHTML=`<header class="folder-header"><div><span>설치 위치</span><h2>설치 폴더 선택</h2></div><button type="button" class="icon-button" data-folder-close aria-label="폴더 창 닫기">${icon('X')}</button></header><form class="folder-location"><button type="button" class="button small" data-folder-up aria-label="상위 폴더">${icon('ArrowUp')}</button><label><span class="sr-only">상위 폴더 경로</span><input name="folderPath" spellcheck="false" placeholder="/Users/you"></label><button type="submit" class="button small">이동</button></form><div class="folder-tools"><button type="button" data-folder-home>${icon('Home')}홈 폴더</button><label><input type="checkbox" data-folder-hidden>숨김 폴더 표시</label></div><p class="folder-error" role="status" hidden></p><div class="folder-items" aria-label="하위 폴더"></div><div class="install-folder-name"><label>새 설치 폴더 이름<input data-folder-name spellcheck="false" value="${escape(folderName)}"></label><small>선택한 위치에 새 폴더를 만들어 설치합니다.</small></div><footer class="folder-footer"><span class="mono" data-folder-path></span><div><button type="button" class="button" data-folder-close>취소</button><button type="button" class="button primary" data-folder-use disabled>이 경로 사용</button></div></footer>`;
      document.body.append(this.dialog);
      this.dialog.querySelector('form').onsubmit=event=>{event.preventDefault();this.load(this.dialog.querySelector('[name="folderPath"]').value);};
      this.dialog.addEventListener('click',event=>{
        const button=event.target.closest('button');if(!button)return;
        if(button.hasAttribute('data-folder-close'))this.dialog.close();
        if(button.hasAttribute('data-folder-up'))this.load(this.parent);
        if(button.hasAttribute('data-folder-home'))this.load(this.home);
        if(button.dataset.folderPath)this.load(button.dataset.folderPath);
        if(button.hasAttribute('data-folder-use')){const destination=this.destination();if(destination){this.onChoose(destination);this.dialog.close();}}
      });
      this.dialog.querySelector('[data-folder-hidden]').onchange=()=>this.load(this.path);
      this.dialog.querySelector('[data-folder-name]').oninput=()=>this.updateDestination();
      this.dialog.addEventListener('close',()=>{this.controller?.abort();this.dialog.remove();},{once:true});
      this.dialog.showModal();this.load(path,true);
    }

    async load(path,nearest=false) {
      this.isLoaded=false;
      this.controller?.abort();this.controller=new AbortController();const controller=this.controller;
      const use=this.dialog.querySelector('[data-folder-use]'),items=this.dialog.querySelector('.folder-items'),error=this.dialog.querySelector('.folder-error');
      use.disabled=true;error.hidden=true;items.innerHTML='<p class="folder-empty">폴더를 불러오는 중…</p>';
      try {
        const parameters=new URLSearchParams({path:path||'',nearest:nearest?'1':'0',hidden:this.dialog.querySelector('[data-folder-hidden]').checked?'1':'0'});
        const response=await fetch(`/__preview__/directories?${parameters}`,{signal:controller.signal}),data=await response.json();
        if(!response.ok)throw new Error(data.error||'폴더를 불러오지 못했습니다.');
        if(controller!==this.controller)return;
        this.path=data.path;this.parent=data.parent;this.home=data.home;this.isLoaded=true;
        this.dialog.querySelector('[name="folderPath"]').value=this.path;
        this.dialog.querySelector('[data-folder-up]').disabled=this.path===this.parent;
        items.innerHTML=data.folders.length?data.folders.map(folder=>`<button type="button" data-folder-path="${escape(folder.path)}">${this.icon('Folder')}<span>${escape(folder.name)}</span>${this.icon('ChevronRight')}</button>`).join(''):'<p class="folder-empty">하위 폴더가 없습니다. 이 위치를 사용할 수 있습니다.</p>';
        if(data.total>data.folders.length)items.insertAdjacentHTML('beforeend','<p class="folder-empty">500개 폴더를 표시했습니다. 위 경로 입력으로 이동할 수 있습니다.</p>');
        this.updateDestination();
      } catch(reason) {
        if(controller.signal.aborted)return;
        this.isLoaded=false;items.innerHTML='';error.textContent=reason.message;error.hidden=false;
      }
    }

    destination() {
      const name=this.dialog.querySelector('[data-folder-name]').value.trim();
      return this.isLoaded&&name&&!/[\/\\\0]/.test(name)&&name!=='.'&&name!=='..'?`${this.path==='/'?'':this.path}/${name}`:null;
    }

    updateDestination() {
      const destination=this.destination(),label=this.dialog.querySelector('.folder-footer [data-folder-path]');
      label.textContent=destination||'설치 폴더 이름을 입력하세요.';label.title=destination||'';
      this.dialog.querySelector('[data-folder-use]').disabled=!destination;
    }
  }
  window.SupervisorInstallFolderDialog=InstallFolderDialog;
})();
