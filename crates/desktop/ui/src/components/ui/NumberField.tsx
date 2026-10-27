import {useEffect,useId,useState} from "react";
import {useFormDrafts} from "./FormDrafts";

export function NumberField({name,value,onValueChange,min,max,integer=true,allowEmpty=false,disabled=false,placeholder}:{
  name:string;value:number|undefined;onValueChange:(value:number|undefined)=>void;
  min?:number;max?:number;integer?:boolean;allowEmpty?:boolean;disabled?:boolean;placeholder?:string;
}) {
  const store=useFormDrafts();const errorId=useId();
  const source=JSON.stringify(value??null);
  const cached=store?.fields[name];
  const [local,setLocal]=useState(String(value??""));
  const raw=cached?.source===source?String(cached.raw):store?String(value??""):local;
  const empty=raw.trim()==="";
  const parsed=Number(raw);
  const error=empty?(allowEmpty?"":"값을 입력하세요.")
    :!/^[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[-+]?\d+)?$/i.test(raw.trim())||!Number.isFinite(parsed)?"숫자를 입력하세요."
    :integer&&!Number.isInteger(parsed)?"정수를 입력하세요."
    :min!==undefined&&parsed<min?`${min} 이상이어야 합니다.`
    :max!==undefined&&parsed>max?`${max} 이하여야 합니다.`:"";
  useEffect(()=>{store?.set(name,{raw,source,isValid:disabled||!error});},[name,raw,source,error,disabled]);
  function change(next:string) {
    setLocal(next);
    const isEmpty=next.trim()==="";const number=Number(next);
    const isValid=isEmpty?allowEmpty:/^[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[-+]?\d+)?$/i.test(next.trim())&&Number.isFinite(number)&&(!integer||Number.isInteger(number))&&(min===undefined||number>=min)&&(max===undefined||number<=max);
    store?.set(name,{raw:next,source:JSON.stringify(isValid?(isEmpty?null:number):value??null),isValid});
    if(isValid)onValueChange(isEmpty?undefined:number);
  }
  return <><input type="text" inputMode={integer?"numeric":"decimal"} name={name} value={raw} disabled={disabled} placeholder={placeholder} aria-invalid={!disabled&&Boolean(error)} aria-describedby={!disabled&&error?errorId:undefined} onChange={event=>change(event.target.value)}/>{!disabled&&error&&<small id={errorId} role="alert" className="text-danger">{error}</small>}</>;
}
