/* Script help is isolated from runtime help and follows the selected file/version/cwd. */
(() => {
  class ScriptHelp {
    constructor({source,getRequest,onChange}) {
      this.source=source;this.form=source.form;this.getRequest=getRequest;this.onChange=onChange;this.key='';this.result=null;
      this.onInput=event=>{if(event.target===source||['cwd','environment'].includes(event.target.name))this.schedule();};
      this.form?.addEventListener('input',this.onInput);
    }

    schedule(refresh=false) {
      const request=this.getRequest(this.result);
      if(!request){clearTimeout(this.timer);this.controller?.abort();this.key='';this.result=null;this.onChange({status:'idle',options:{},reason:'실행할 파일이나 CLI 명령을 지정하면 도움말을 읽습니다.'});return;}
      const key=request.key;
      if(!refresh&&key===this.key)return;
      clearTimeout(this.timer);
      this.key=key;this.controller?.abort();this.result=null;
      this.onChange({status:'loading',options:{},reason:'스크립트 / CLI 도움말을 읽고 있습니다.'});
      this.timer=setTimeout(()=>this.load(request,refresh),refresh?0:500);
    }

    async load(request,refresh) {
      const controller=new AbortController();this.controller=controller;
      const timeout=setTimeout(()=>controller.abort(),20000);
      try {
        const {key,...payload}=request;
        const response=await fetch('/__preview__/application-help',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...payload,refresh}),signal:controller.signal,cache:'no-store'});
        if(!response.ok)throw new Error('help unavailable');
        const result=await response.json();
        if(this.destroyed||controller!==this.controller||key!==this.key)return;
        this.result=result;this.onChange(result);
      } catch(error) {
        if(!this.destroyed&&controller===this.controller&&request.key===this.key)this.onChange({status:'unavailable',options:{},reason:'도움말을 읽을 수 없습니다. 직접 입력할 수 있습니다.'});
      } finally {clearTimeout(timeout);}
    }

    destroy() {this.destroyed=true;clearTimeout(this.timer);this.controller?.abort();this.form?.removeEventListener('input',this.onInput);}
  }
  window.SupervisorScriptHelp=ScriptHelp;
})();
