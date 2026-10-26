export const resultSources={exit_code:'종료 코드',stdout:'표준 출력',stderr:'오류 출력',outcome:'실행 상태'};
export const resultOperators={equals:'같음',not_equals:'다름',gte:'이상',lte:'이하',includes:'포함',not_includes:'포함하지 않음'};
export const outcomeLabels={succeeded:'정상 종료',failed:'실행 실패',timeout:'시간 초과',circuit_open:'서킷 차단'};
export const supportsResultRoutes=data=>['command','process','health','job'].includes(data.kind);
export const resultRoutes=data=>supportsResultRoutes(data)&&Array.isArray(data.resultRoutes)?data.resultRoutes:[];
export const operatorsFor=source=>source==='outcome'?['equals']:source==='exit_code'?['equals','not_equals','gte','lte']:['includes','not_includes','equals','not_equals'];
export function routeConditionLabel(route) {
  return route.source==='outcome'?outcomeLabels[route.value]||'실행 상태 선택':`${resultSources[route.source]||'결과'} ${resultOperators[route.operator]||'같음'} ${route.value===''?'(빈 값)':route.value}`;
}
export function sourcePorts(data) {
  if(data.kind==='finish')return [];
  if(data.kind==='parallel')return (data.branches||[]).map(branch=>({...branch,condition:'동시에 실행'}));
  if(data.kind==='condition')return [{id:'true',label:'참일 때'},{id:'false',label:'거짓일 때'}];
  const routes=resultRoutes(data);
  return [...routes.map(route=>({id:route.id,label:route.label,condition:routeConditionLabel(route),changeStatus:route.changeStatus})),{id:'out',label:routes.length?'그 외 결과':'다음 단계'}];
}
export function resultRoutingError(data) {
  const routes=resultRoutes(data),ids=new Set();
  for(const route of routes) {
    if(!route.id||route.id==='out'||ids.has(route.id))return '결과 경로의 식별자가 올바르지 않습니다.';
    ids.add(route.id);
    if(!route.label?.trim())return '결과 경로 이름을 입력하세요.';
    if(!resultSources[route.source]||!operatorsFor(route.source).includes(route.operator))return `${route.label}: 비교할 결과와 조건을 선택하세요.`;
    if(route.source==='exit_code'&&!/^-?\d+$/.test(String(route.value)))return `${route.label}: 종료 코드를 정수로 입력하세요.`;
    if(route.source==='outcome'&&!outcomeLabels[route.value])return `${route.label}: 실행 상태를 선택하세요.`;
    if(['includes','not_includes'].includes(route.operator)&&!String(route.value).length)return `${route.label}: 찾을 문자열을 입력하세요.`;
  }
  return '';
}
export function selectResultRoute(data,result) {
  const outcome=result.outcome||(result.exitCode===0?'succeeded':'failed');
  const matched=resultRoutes(data).find(route=>{
    const actual=route.source==='exit_code'?result.exitCode:route.source==='outcome'?outcome:result[route.source]||'';
    if(actual===null||actual===undefined)return false;
    const expected=route.source==='exit_code'?Number(route.value):String(route.value);
    if(route.operator==='not_equals')return actual!==expected;
    if(route.operator==='gte')return actual>=expected;
    if(route.operator==='lte')return actual<=expected;
    if(route.operator==='includes')return String(actual).includes(expected);
    if(route.operator==='not_includes')return !String(actual).includes(expected);
    return actual===expected;
  });
  return matched?{id:matched.id,label:matched.label,condition:routeConditionLabel(matched)}:{id:'out',label:'그 외 결과',condition:'어떤 조건에도 일치하지 않음'};
}
