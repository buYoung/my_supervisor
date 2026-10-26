const circuits=new Map();
const keyFor=(jobId,stepId)=>`${jobId}:${stepId}`;
const initial=()=>({phase:'closed',failures:0,successes:0,openUntil:0,isProbing:false});
function read(key){if(!circuits.has(key)){try{circuits.set(key,JSON.parse(sessionStorage.getItem(`msv-circuit-${key}`))||initial());}catch{circuits.set(key,initial());}}return circuits.get(key);}
function publish(key,value){circuits.set(key,value);try{sessionStorage.setItem(`msv-circuit-${key}`,JSON.stringify({...value,isProbing:false}));}catch{}window.dispatchEvent(new CustomEvent('supervisor-circuit-changed',{detail:{key}}));}
export const circuitLabels={closed:'정상 통과',open:'호출 차단',half_open:'복구 확인'};
export function circuitState(jobId,stepId){const value=read(keyFor(jobId,stepId));return {...value,phase:value.phase==='open'&&value.openUntil<=Date.now()?'half_open':value.phase};}
export function resetCircuit(jobId,stepId){publish(keyFor(jobId,stepId),initial());}
export function acquireCircuit(jobId,stepId,policy){
  if(!policy?.enabled)return {allowed:true,enabled:false};
  const key=keyFor(jobId,stepId),value=circuitState(jobId,stepId);
  if(value.phase==='open'||value.phase==='half_open'&&value.isProbing)return {allowed:false,phase:value.phase,remainingSeconds:Math.max(0,Math.ceil((value.openUntil-Date.now())/1000))};
  if(value.phase==='half_open'){value.isProbing=true;publish(key,value);}
  return {allowed:true,enabled:true,phase:value.phase};
}
export function settleCircuit(jobId,stepId,policy,outcome){
  if(!policy?.enabled)return;
  const key=keyFor(jobId,stepId),value=circuitState(jobId,stepId);value.isProbing=false;
  if(outcome==='cancelled'){publish(key,value);return;}
  if(outcome==='succeeded'){
    if(value.phase==='half_open'&&value.successes+1<Number(policy.recoverySuccesses||1)){value.successes++;publish(key,value);}
    else publish(key,initial());
    return;
  }
  value.failures++;value.successes=0;
  if(value.phase==='half_open'||value.failures>=Number(policy.failureThreshold||3)){value.phase='open';value.openUntil=Date.now()+Number(policy.openSeconds||30)*1000;}
  publish(key,value);
}
export function retryDelaySeconds(data,failedAttempt,withJitter=false){
  const base=Number(data.retryDelaySeconds)||1,strategy=data.retryStrategy||'fixed';
  const value=strategy==='linear'?base+(failedAttempt-1)*(Number(data.retryIncrementSeconds)||1):strategy==='exponential'?base*Math.pow(Number(data.retryMultiplier)||2,failedAttempt-1):base;
  const capped=Math.min(value,Number(data.retryMaxDelaySeconds)||60);
  return withJitter&&data.retryJitter?capped*(.8+Math.random()*.4):capped;
}
export function policyError(data){
  if(data.maxAttempts!==undefined&&(!Number.isInteger(Number(data.maxAttempts))||Number(data.maxAttempts)<1||Number(data.maxAttempts)>10))return '총 시도 횟수는 1~10회로 입력하세요.';
  if(data.timeoutSeconds!==undefined&&data.timeoutSeconds!==''&&(!Number.isInteger(Number(data.timeoutSeconds))||Number(data.timeoutSeconds)<1))return '제한 시간은 1초 이상의 정수로 입력하세요.';
  if(Number(data.maxAttempts)>1){
    for(const key of ['retryDelaySeconds',...(data.retryStrategy==='linear'?['retryIncrementSeconds']:[])])if(!Number.isFinite(Number(data[key]??1))||Number(data[key]??1)<=0)return '재시도 간격과 증가량은 0보다 크게 입력하세요.';
    if(data.retryStrategy==='exponential'&&(!Number.isFinite(Number(data.retryMultiplier||2))||Number(data.retryMultiplier||2)<=1))return '지수 증가 배수는 1보다 커야 합니다.';
    if(!Number.isFinite(Number(data.retryMaxDelaySeconds??60))||Number(data.retryMaxDelaySeconds??60)<Number(data.retryDelaySeconds||1))return '최대 재시도 간격은 첫 간격보다 작을 수 없습니다.';
  }
  if(data.circuitBreaker?.enabled){for(const key of ['failureThreshold','openSeconds','recoverySuccesses'])if(!Number.isInteger(Number(data.circuitBreaker[key]))||Number(data.circuitBreaker[key])<1)return '서킷 브레이커의 횟수와 차단 시간은 1 이상의 정수로 입력하세요.';}
  return '';
}
