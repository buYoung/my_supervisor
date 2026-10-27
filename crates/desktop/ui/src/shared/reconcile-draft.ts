export interface DraftConflict {path:string;draft:unknown;saved:unknown}
export type DraftChoices=Record<string,"draft"|"saved">;
export function stableJson(value:unknown):string{return JSON.stringify(value,(_,entry)=>entry&&typeof entry==="object"&&!Array.isArray(entry)?Object.fromEntries(Object.entries(entry).sort(([a],[b])=>a.localeCompare(b))):entry)??"undefined";}
const isRecord=(value:unknown):value is Record<string,unknown>=>Boolean(value)&&typeof value==="object"&&!Array.isArray(value);
/** Reconcile independent fields; overlapping edits require an explicit choice. */
export function reconcileDraft<T>(base:T,draft:T,saved:T,choices:DraftChoices={}){
  const conflicts:DraftConflict[]=[];
  function merge(base:unknown,draft:unknown,saved:unknown,path:string):unknown{
    if(stableJson(draft)===stableJson(base))return saved;
    if(stableJson(saved)===stableJson(base)||stableJson(draft)===stableJson(saved))return draft;
    if(isRecord(base)&&isRecord(draft)&&isRecord(saved)){
      const result:Record<string,unknown>={};
      for(const key of new Set([...Object.keys(base),...Object.keys(draft),...Object.keys(saved)])){
        const value=merge(base[key],draft[key],saved[key],path+"/"+key.replaceAll("~","~0").replaceAll("/","~1"));
        if(value!==undefined)result[key]=value;
      }
      return result;
    }
    if(Array.isArray(base)&&Array.isArray(draft)&&Array.isArray(saved)){
      const ids=(items:unknown[])=>items.every(item=>isRecord(item)&&typeof item.id==="string")?items.map(item=>(item as {id:string}).id):undefined;
      const baseIds=ids(base),draftIds=ids(draft),savedIds=ids(saved);
      if(baseIds&&draftIds&&savedIds&&stableJson(baseIds)===stableJson(draftIds)&&stableJson(baseIds)===stableJson(savedIds))return draft.map((item,index)=>merge(base[index],item,saved[index],path+"/"+encodeURIComponent(draftIds[index])));
    }
    conflicts.push({path:path||"/",draft,saved});
    return choices[path||"/"]==="saved"?saved:draft;
  }
  return {value:merge(base,draft,saved,"") as T,conflicts};
}
