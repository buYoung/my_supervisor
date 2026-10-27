export const schedulePresets=[
  {id:"hourly",label:"매시간 정각",expression:"0 * * * *"},
  {id:"daily",label:"매일 오전 9시",expression:"0 9 * * *"},
  {id:"weekdays",label:"평일 오전 9시",expression:"0 9 * * 1-5"},
  {id:"weekly",label:"매주 월요일 오전 9시",expression:"0 9 * * 1"},
];
export function describeCron(expression:string):string{
  const exact=schedulePresets.find(item=>item.expression===expression.trim());if(exact)return exact.label;
  const fields=expression.trim().split(/\s+/);if(fields.length!==5)return "분·시·일·월·요일의 5개 값을 입력하고 미리보기로 확인하세요.";
  const [minute,hour,day,month,week]=fields;
  if(/^\d+$/.test(minute)&&/^\d+$/.test(hour)&&Number(minute)<60&&Number(hour)<24&&day==="*"&&month==="*"&&week==="*")return `매일 ${hour.padStart(2,"0")}:${minute.padStart(2,"0")}`;
  if(/^\*\/\d+$/.test(minute)&&Number(minute.slice(2))>0&&Number(minute.slice(2))<60&&hour==="*"&&day==="*"&&month==="*"&&week==="*")return `매시간 0분부터 ${minute.slice(2)}분 단위`;
  return "직접 입력한 cron입니다. 정확한 실행 시각은 서버 미리보기에서 확인하세요.";
}
