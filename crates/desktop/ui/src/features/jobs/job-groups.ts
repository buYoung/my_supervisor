import type {JobStatus} from "../../shared/types";
export function connectedJobGroups(jobs:JobStatus[],metadata:Record<string,{name:string;members:string[]}>) {
 const adjacent=new Map<string,Set<string>>();
 const connect=(left:string,right:string)=>{if(!adjacent.has(left))adjacent.set(left,new Set());if(!adjacent.has(right))adjacent.set(right,new Set());adjacent.get(left)!.add(right);adjacent.get(right)!.add(left);};
 for(const job of jobs){if(!adjacent.has(job.name))adjacent.set(job.name,new Set());for(const name of [...job.dependencies.upstream,...job.dependencies.downstream,...metadata[job.name]?.members||[]])connect(job.name,name);}
 const groups=new Map<string,string[]>();
 for(const name of adjacent.keys()){if(groups.has(name))continue;const pending=[name],members=new Set<string>();while(pending.length){const current=pending.pop()!;if(members.has(current))continue;members.add(current);pending.push(...adjacent.get(current)||[]);}const sorted=[...members].sort();for(const member of sorted)groups.set(member,sorted);}
 return groups;
}
