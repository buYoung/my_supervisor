import type {OperationsClient,ResourcePage} from "./operations-client";

export interface ResourceSnapshot<T>{data:T|null;error:unknown|null;isLoading:boolean;isRefreshing:boolean;updatedAt:number|null}
export type RefreshResult="success"|"error"|"superseded";
export class ResourceQuery<T>{
  private snapshot:ResourceSnapshot<T>={data:null,error:null,isLoading:true,isRefreshing:false,updatedAt:null};
  private subscribers=new Map<()=>void,number>();
  private timer:ReturnType<typeof setInterval>|undefined;
  private generation=0;
  private pending:Promise<RefreshResult>|undefined;
  constructor(private fetcher:()=>Promise<T>){}
  getSnapshot=()=>this.snapshot;
  get isActive(){return this.subscribers.size>0;}
  setFetcher(fetcher:()=>Promise<T>){this.fetcher=fetcher;}
  private publish(next:ResourceSnapshot<T>){this.snapshot=next;for(const listener of this.subscribers.keys())listener();}
  subscribe=(listener:()=>void,intervalMs:number)=>{
    const wasInactive=!this.isActive;this.subscribers.set(listener,intervalMs);this.configureTimer();if(wasInactive)void this.refresh();
    return()=>{this.subscribers.delete(listener);this.configureTimer();};
  };
  private configureTimer(){
    if(this.timer)clearInterval(this.timer);
    const intervals=[...this.subscribers.values()].filter(value=>value>0);
    this.timer=intervals.length?setInterval(()=>void this.refresh(),Math.min(...intervals)):undefined;
  }
  refresh=(force=false):Promise<RefreshResult>=>{
    if(this.pending&&!force)return this.pending;
    const generation=++this.generation;
    this.publish({...this.snapshot,isLoading:this.snapshot.data===null,isRefreshing:true});
    const pending=(async():Promise<RefreshResult>=>{
      try{const data=await this.fetcher();if(generation!==this.generation)return "superseded";this.publish({data,error:null,isLoading:false,isRefreshing:false,updatedAt:Date.now()});return "success";}
      catch(error){if(generation!==this.generation)return "superseded";this.publish({...this.snapshot,error,isLoading:false,isRefreshing:false});return "error";}
      finally{if(generation===this.generation)this.pending=undefined;}
    })();
    this.pending=pending;return pending;
  };
}
const caches=new WeakMap<OperationsClient,Map<string,ResourceQuery<unknown>>>();
function cacheFor(client:OperationsClient){let cache=caches.get(client);if(!cache){cache=new Map();caches.set(client,cache);}return cache;}
export function resourceQuery<T>(client:OperationsClient,key:string,fetcher:()=>Promise<T>):ResourceQuery<T>{
  const cache=cacheFor(client);let query=cache.get(key) as ResourceQuery<T>|undefined;
  if(!query){query=new ResourceQuery(fetcher);cache.set(key,query as ResourceQuery<unknown>);}
  else query.setFetcher(fetcher);
  if(cache.size>256)for(const [entryKey,entry] of cache){if(cache.size<=256)break;if(!entry.isActive&&entryKey!==key&&!["processes","jobs","daemon"].includes(entryKey))cache.delete(entryKey);}
  return query;
}
export async function refreshResources(client:OperationsClient,accept:(key:string)=>boolean=()=>true){
  return Promise.all([...cacheFor(client)].filter(([key,query])=>query.isActive&&accept(key)).map(async([key,query])=>({key,result:await query.refresh(true)})));
}
export interface PagedSnapshot<T>{items:T[];hasMore:boolean;partial:boolean;failedPartitions:string[];highWatermark:string;pages:number}
interface PageController<T>{query:ResourceQuery<PagedSnapshot<T>>;loadMore:()=>Promise<RefreshResult>}
const pages=new WeakMap<OperationsClient,Map<string,PageController<unknown>>>();
export function pagedQuery<T>(client:OperationsClient,key:string,fetchPage:(cursor?:string,watermark?:string,limit?:number)=>Promise<ResourcePage<T>>,identity:(item:T)=>string):PageController<T>{
  let cache=pages.get(client);if(!cache){cache=new Map();pages.set(client,cache);}const existing=cache.get(key);if(existing)return existing as PageController<T>;
  let requestedPages=1;
  const query=resourceQuery(client,key,async()=>{
    const items=new Map<string,T>(),failed=new Set<string>();let cursor:string|undefined,watermark:string|undefined,partial=false,count=0;
    do{
      const page=await fetchPage(cursor,watermark,100);if(watermark!==undefined&&watermark!==page.highWatermark)throw new Error("목록 스냅샷이 바뀌었습니다. 다시 새로고침하세요.");
      watermark=page.highWatermark;for(const item of page.records)items.set(identity(item),item);for(const partition of page.failedPartitions)failed.add(partition);partial ||= page.partial;cursor=page.nextCursor;count++;
    }while(cursor&&count<requestedPages);
    return {items:[...items.values()],hasMore:Boolean(cursor),partial,failedPartitions:[...failed],highWatermark:watermark??"",pages:count};
  });
  const controller={query,loadMore:()=>{requestedPages++;return query.refresh(true);}};cache.set(key,controller as PageController<unknown>);return controller;
}
