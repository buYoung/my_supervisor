import type {ProcessConfigDto} from "../../services/wire-types";

export function processValidation(config:ProcessConfigDto):string[] {
  const errors:string[]=[];
  const watch=config.watch;
  if(watch){
    if(!watch.roots.length||watch.roots.length>256||watch.roots.some(path=>!path.startsWith("/")))errors.push("감시할 절대 경로를 1~256개 입력하세요.");
    if(watch.exclusions.some(path=>!path.startsWith("/")))errors.push("제외 경로는 절대 경로의 접두사로 입력하세요.");
    if(watch.debounce_ms<=0)errors.push("변경 모으기는 0보다 커야 합니다.");
    if(watch.follow_symlinks)errors.push("심볼릭 링크 따라가기는 지원하지 않습니다. 해제한 뒤 저장하세요.");
  }
  for(const [name,check] of [["응답 확인",config.liveness],["준비 상태",config.readiness]] as const){
    if(check&&check.timeout_ms>=check.interval_ms)errors.push(`${name}: 응답 대기는 확인 간격보다 짧아야 합니다.`);
    if(check?.kind.type==="exec"&&!check.kind.command.trim())errors.push(`${name}: 실행 파일을 입력하세요.`);
  }
  if(config.rolling){
    if(!config.readiness)errors.push("순차 재시작에는 준비 상태 검사가 필요합니다.");
    if(config.rolling.max_unavailable>=(config.instances??1))errors.push("동시에 중지할 수 있는 수는 인스턴스 수보다 작아야 합니다.");
    if(config.rolling.max_surge<1||config.rolling.readiness_timeout_ms<=0)errors.push("추가 실행 수와 준비 상태 대기는 0보다 커야 합니다.");
  }
  return errors;
}
