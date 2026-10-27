import type {LogEntry} from '../../services/log-mapping';
export interface LogNavigation {kind:'process'|'job';source:string;runId:string;step:string;level:string;stream:string;search:string;range:string;from:string;to:string;grouping:'repeat'|'none';ascending:boolean;offset:number;snapshot?:string;scrollTop:number;focusId:string}
export interface LogContext {id:string;kind:'process'|'job';source:string;runId?:string;timestamp:string}
export function parseLogNavigation(params:URLSearchParams):LogNavigation{
 const isScoped=params.has('source')||params.has('run')||params.has('step');
 return {kind:params.get('kind')==='job'?'job':'process',source:params.get('source')==='all'?'':params.get('source')??'',runId:params.get('run')??'',step:params.get('step')??'',level:params.get('level')??(isScoped?'all':'attention'),stream:params.get('stream')??'',search:params.get('search')??'',range:['1','7','30','210','all','custom'].includes(params.get('range')??'')?params.get('range')!:(isScoped?'all':'7'),from:params.get('from')??'',to:params.get('to')??'',grouping:params.get('grouping')==='none'?'none':'repeat',ascending:params.get('order')==='asc',offset:Math.max(0,Number(params.get('offset'))||0),snapshot:params.get('snapshot')||undefined,scrollTop:Math.max(0,Number(params.get('position'))||0),focusId:params.get('focus')??''};
}
export function logNavigationUrl(nav:LogNavigation,href:string):URL{
 const url=new URL(href);url.searchParams.set('page','logs');
 for(const [key,value] of Object.entries({kind:nav.kind,source:nav.source||'all',run:nav.runId,step:nav.step,level:nav.level,stream:nav.stream,search:nav.search,range:nav.range,from:nav.from,to:nav.to,grouping:nav.grouping,order:nav.ascending?'asc':'desc',offset:String(nav.offset),snapshot:nav.snapshot??'',position:String(nav.scrollTop),focus:nav.focusId})){if(value)url.searchParams.set(key,value);else url.searchParams.delete(key);}
 return url;
}
export function logEntryUrl(entry:Pick<LogEntry,'kind'|'source'|'runId'|'step'>,around?:string){const params=new URLSearchParams({page:'logs',kind:entry.kind,source:entry.source,level:'all',range:'all'});if(entry.runId)params.set('run',entry.runId);if(entry.step)params.set('step',entry.step);if(around)params.set('around',around);return '?'+params.toString();}
export function contextFor(entry:LogEntry):LogContext{return {id:entry.id,kind:entry.kind,source:entry.source,runId:entry.runId,timestamp:entry.raw.timestamp};}
export function parseLogContext(params:URLSearchParams,nav:LogNavigation):LogContext|null{const timestamp=params.get('around');if(!timestamp||!Number.isFinite(Date.parse(timestamp)))return null;return {id:params.get('context')??timestamp,kind:params.get('contextKind')==='job'?'job':params.has('contextKind')?'process':nav.kind,source:params.get('contextSource')??nav.source,runId:params.get('contextRun')||nav.runId||undefined,timestamp};}
