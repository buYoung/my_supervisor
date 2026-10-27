import {createContext,useCallback,useContext,useMemo,useState,type ReactNode} from "react";

export interface FieldDraft {raw:unknown;source:string;isValid:boolean}
interface FormDraftStore {
  fields:Record<string,FieldDraft>;
  set:(key:string,draft:FieldDraft)=>void;
  clear:()=>void;
  replace:(fields:Record<string,FieldDraft>)=>void;
  hasInvalid:(isActive?:(key:string)=>boolean)=>boolean;
}
const Context=createContext<FormDraftStore|null>(null);

/** Session-only field drafts survive tab changes without entering wire DTOs. */
export function FormDraftsProvider({children}:{children:ReactNode}) {
  const [fields,setFields]=useState<Record<string,FieldDraft>>({});
  const set=useCallback((key:string,draft:FieldDraft)=>setFields(current=>{
    const before=current[key];
    return before&&before.source===draft.source&&before.isValid===draft.isValid&&JSON.stringify(before.raw)===JSON.stringify(draft.raw)?current:{...current,[key]:draft};
  }),[]);
  const value=useMemo(()=>({fields,set,replace:setFields,clear:()=>setFields({}),hasInvalid:(isActive=(key:string)=>Boolean(key))=>Object.entries(fields).some(([key,draft])=>!draft.isValid&&isActive(key))}),[fields,set]);
  return <Context.Provider value={value}>{children}</Context.Provider>;
}
export function useFormDrafts(){return useContext(Context);}
