export interface HelpOption {name:string;description:string;requiresValue:boolean;separator:string;scope:"runtime"|"script"}
export interface ArgumentRow {id:string;name:string;value:string;separator:string;scope:"runtime"|"entry"|"script";isEmptyArgument?:boolean}
export const basename=(path:string)=>path.split(/[\\/]/).pop()||path;
export function splitCommand(line:string):string[]{
  const words:string[]=[];let word="",quote="",escaped=false,started=false;
  for(const char of line){if(escaped){word+=char;escaped=false;started=true;continue;}if(char==="\\"&&quote!=="'"){escaped=true;started=true;continue;}if(quote){if(char===quote)quote="";else word+=char;started=true;continue;}if(char==="'"||char==='"'){quote=char;started=true;}else if(/\s/.test(char)){if(started){words.push(word);word="";started=false;}}else{word+=char;started=true;}}
  if(escaped||quote)throw new Error("따옴표나 이스케이프 문자를 닫아 주세요.");if(started)words.push(word);return words;
}
export function quoteArgument(word:string){return /^[A-Za-z0-9_./:@%+,-]+$/.test(word)?word:`'${word.replaceAll("'","'\\''")}'`;}
export function parseHelp(text:string,scope:"runtime"|"script"):HelpOption[]{
  const result=new Map<string,HelpOption>();
  let previous:string[]=[];
  for(const raw of text.replace(/\x1b\[[0-9;]*m/g,"").split(/\r?\n/)){const line=raw.trim();if(!line.startsWith("-")){if(raw.match(/^\s{4,}\S/)&&line&&previous.length)for(const key of previous){const item=result.get(key)!;item.description+=" "+line;}else previous=[];continue;}
    const parts=line.split(/\s{2,}|\t+/);const prefix=parts[0];const names=prefix.match(/--?[A-Za-z0-9][A-Za-z0-9_.-]*/g)||[];const description=parts.slice(1).join(" ")||"";
    const requiresValue=names.some(name=>/^(?:=|<|\[=|\s+(?:<|\[|[A-Z][A-Z_0-9]+\b|string\b|int\b|float\b|value\b|path\b|number\b))/.test(prefix.slice(prefix.indexOf(name)+name.length)));
    previous=names;for(const name of names){const tail=prefix.slice(prefix.indexOf(name)+name.length);const separator=tail.startsWith("=")?"=":tail.startsWith("<")?"":" ";result.set(name,{name,description:description||name,requiresValue,separator,scope});}}
  if(scope==="runtime")for(const [name,description] of [["-Xmx","Java 최대 힙 메모리 (예: 2048m)"],["-Xms","Java 초기 힙 메모리 (예: 256m)"],["-D","Java 시스템 속성 이름과 값 (예: key=value)"]])if(text.includes(name))result.set(name,{name,description,requiresValue:true,separator:"",scope});
  return [...result.values()];
}
export function helpInvocation(program:string,args:string[]):string[]{
  const name=basename(program);const rows=argumentRows(program,args);const entry=rows.findIndex(row=>row.scope==="entry");
  if(entry>=0)return rowsToArguments(rows.slice(0,entry+1));
  if(/^(git|cargo|docker|kubectl|npm|pnpm|yarn|go|dotnet|deno|uv|poetry|pip[\d.]*)$/.test(name)){
    const commands=args.filter(arg=>!arg.startsWith("-"));if(!commands.length)return [];
    const count=name==="docker"&&commands[0]==="compose"?2:1;return commands.slice(0,count);
  }return [];
}
export function argumentRows(program:string,args:string[],help:HelpOption[]=[]):ArgumentRow[]{
  const result:ArgumentRow[]=[];let hasEntry=false;const interpreter=/^(node|python[\d.]*|bun|java|ruby|deno)$/.test(basename(program));
  for(let i=0;i<args.length;i++){const arg=args[i];let name="",value=arg,separator=" ";let scope:ArgumentRow["scope"]=hasEntry?"script":"runtime";
    const takesValue=(option:string)=>/^(sh|bash|zsh|fish)$/.test(basename(program))&&option==="-c"||/^(node|bun)$/.test(basename(program))&&["-e","-p","-r","--eval","--print","--require","--import","--loader"].includes(option)||/^python[\d.]*$/.test(basename(program))&&["-c","-m","-W","-X"].includes(option)||basename(program)==="java"&&["-cp","-classpath","--class-path","--module-path","-p","-m","--module","-jar"].includes(option);
    if(arg.startsWith("-")&&arg!=="-"){const attached=arg.match(/^(-X(?:mx|ms|ss))(.+)$/);const equals=arg.indexOf("=");if(attached){name=attached[1];value=attached[2];separator="";}else if(equals>0){name=arg.slice(0,equals);value=arg.slice(equals+1);separator="=";}else {name=arg;value="";const option=help.find(o=>o.name===arg);const next=args[i+1];if(next&&(!next.startsWith("-")||/^-\d/.test(next))&&(option?.requiresValue||!option&&!/\.(m?js|cjs|py|jar)$/.test(next)&&!["--inspect","--watch","--help","-h","-v","--version"].includes(arg))){value=next;i++;}}
    }else{scope=interpreter&&!hasEntry?"entry":"script";hasEntry=hasEntry||scope==="entry";}
    if(name&&!value&&takesValue(name)&&args[i+1]){value=args[++i];}
    if(name==="-jar"&&value){scope="entry";hasEntry=true;}
    result.push({id:crypto.randomUUID(),name,value,separator,scope,isEmptyArgument:arg===""});
  }return result;
}
export function rowsToArguments(rows:ArgumentRow[]):string[]{return ["runtime","entry","script"].flatMap(scope=>rows.filter(r=>r.scope===scope&&(r.name||r.value||r.isEmptyArgument)).flatMap(row=>{const separator=/^-X(?:mx|ms|ss)$/.test(row.name)||row.name==="-D"?"":row.name.startsWith("-D")?"=":row.separator;return !row.name?[row.value]:!row.value?[row.name]:separator===" "?[row.name,row.value]:[row.name+separator+row.value];}));}
