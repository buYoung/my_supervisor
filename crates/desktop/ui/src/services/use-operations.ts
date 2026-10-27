import {useCallback,useId,useRef,useSyncExternalStore} from "react";
import {getOperationsClient,OperationsError,type OperationsClient} from "./index";
import {resourceQuery,pagedQuery,refreshResources,type ResourceQuery,type ResourceSnapshot,type RefreshResult} from "./resource-store";

export function useOperationsClient():OperationsClient{const client=useRef<OperationsClient|null>(null);if(!client.current)client.current=getOperationsClient();return client.current;}
export function toErrorMessage(error:unknown):string{return error instanceof Error?error.message:"알 수 없는 오류가 발생했습니다.";}
export interface PolledResource<T>{data:T|null;isLoading:boolean;isRefreshing:boolean;errorMessage:string|null;isStale:boolean;updatedAt:number|null;refresh:()=>Promise<RefreshResult>}
function useQuery<T>(query:ResourceQuery<T>,intervalMs:number):PolledResource<T>{
  const subscribe=useCallback((listener:()=>void)=>query.subscribe(listener,intervalMs),[query,intervalMs]);
  const snapshot:ResourceSnapshot<T>=useSyncExternalStore(subscribe,query.getSnapshot,query.getSnapshot);
  const refresh=useCallback(()=>query.refresh(true),[query]);
  return {data:snapshot.data,isLoading:snapshot.isLoading,isRefreshing:snapshot.isRefreshing,errorMessage:snapshot.error===null?null:toErrorMessage(snapshot.error),isStale:snapshot.data!==null&&snapshot.error!==null,updatedAt:snapshot.updatedAt,refresh};
}
export function useResource<T>(key:string,fetcher:()=>Promise<T>,intervalMs=0):PolledResource<T>{const client=useOperationsClient();return useQuery(resourceQuery(client,key,fetcher),intervalMs);}
export function usePolledResource<T>(fetcher:()=>Promise<T>,intervalMs:number):PolledResource<T>{const id=useId();return useResource("local:"+id,fetcher,intervalMs);}
export function useDaemonResource(){const client=useOperationsClient();return useResource("daemon",()=>client.daemonStatus(),2000);}
export function useProcessResource(){const client=useOperationsClient();const page=pagedQuery(client,"processes",(...args)=>client.listProcessesPage(...args),item=>item.name);const value=useQuery(page.query,2000);return {...value,data:value.data?.items??null,hasMore:value.data?.hasMore??false,partial:value.data?.partial??false,failedPartitions:value.data?.failedPartitions??[],pages:value.data?.pages??0,loadMore:page.loadMore};}
export function useJobResource(){const client=useOperationsClient();const page=pagedQuery(client,"jobs",(...args)=>client.listJobsPage(...args),item=>item.name);const value=useQuery(page.query,2000);return {...value,data:value.data?.items??null,hasMore:value.data?.hasMore??false,partial:value.data?.partial??false,failedPartitions:value.data?.failedPartitions??[],pages:value.data?.pages??0,loadMore:page.loadMore};}
export {refreshResources,OperationsError};
