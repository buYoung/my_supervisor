import type {ProcessConfigDto} from "../../services/wire-types";
import {PolicyToggle, RestartGuards} from "./GuardControls";

export function RestartControls({config, onChange, onOpenHealth}: {
  config: ProcessConfigDto;
  onChange: (value: Partial<ProcessConfigDto>) => void;
  onOpenHealth: () => void;
}) {
  const isSystemManaged = config.management_mode?.type === "system_registered";
  const isExitRestartEnabled = config.restart?.enabled ?? true;
  const hasRestartCondition = isExitRestartEnabled || Boolean(config.watch || config.memory || config.liveness);
  const updateRestart = (value: Partial<NonNullable<ProcessConfigDto["restart"]>>) => onChange({restart: {...config.restart, ...value}});

  return <div className="restart-controls">
    <div className="settings-section-heading"><h3>재시작 조건</h3><p>종료 감지와 실행 중 감시 조건을 한곳에서 설정합니다.</p></div>
    <PolicyToggle title="프로세스 종료" description="정상 종료(코드 0)를 포함해 프로세스가 끝나면 다시 시작합니다. 직접 ‘중지’를 누른 경우에는 재시작하지 않습니다."
      enabled={isExitRestartEnabled} effect={isSystemManaged ? "macOS가 다시 시작" : "종료 시 재시작"} onChange={enabled => updateRestart({enabled})}/>
    {isSystemManaged ? <p className="policy-info">시스템 서비스는 macOS가 재시작을 관리합니다. 파일·메모리 감시와 아래 재시도 간격은 직접 관리 방식에서 사용할 수 있습니다.</p> : <>
      <RestartGuards config={config} onChange={onChange}/>
      <section className="policy-card policy-linked-setting">
        <div><strong>응답 확인 실패</strong><p>{config.liveness ? "응답 확인이 " + config.liveness.consecutive_failures + "회 연속 실패하면 재시작합니다." : "실행 중인 프로세스가 응답하지 않을 때도 재시작할 수 있습니다."}</p></div>
        <div><span className="policy-effect">{config.liveness ? "사용 중" : "사용 안 함"}</span><button type="button" onClick={onOpenHealth}>상태 확인에서 설정 →</button></div>
      </section>
    </>}

    <details className="restart-timing" open>
      <summary><span>재시작 간격과 횟수</span><small>{isSystemManaged ? "macOS에서 관리" : "최초 " + (config.restart?.backoff_initial_ms ?? 1000) / 1000 + "초 대기 · " + (config.restart?.max_retries == null ? "횟수 제한 없음" : "최대 " + config.restart.max_retries + "회")}</small></summary>
      <fieldset disabled={isSystemManaged || !hasRestartCondition}>
        <p className="editor-note">{hasRestartCondition ? "켜둔 자동 재시작 조건에 공통으로 적용합니다. 반복 실패로 인한 무한 재시작을 조절합니다." : "자동 재시작 조건을 켜면 아래 설정이 적용됩니다."}</p>
        <div className="editor-columns">
          <label className="editor-field"><span>처음 재시작하기 전 대기 (ms)</span><input type="number" min={0} value={config.restart?.backoff_initial_ms ?? 1000} onChange={event => updateRestart({backoff_initial_ms: Number(event.target.value)})}/></label>
          <label className="editor-field"><span>반복 실패 시 최대 대기 (ms)</span><input type="number" min={0} value={config.restart?.backoff_max_ms ?? 60000} onChange={event => updateRestart({backoff_max_ms: Number(event.target.value)})}/></label>
          <label className="editor-field"><span>실패할 때마다 대기 시간 증가 배수</span><input type="number" min={1} value={config.restart?.backoff_multiplier ?? 2} onChange={event => updateRestart({backoff_multiplier: Number(event.target.value)})}/></label>
          <label className="editor-field"><span>최대 재시작 횟수</span><input type="number" min={0} placeholder="제한 없음" value={config.restart?.max_retries ?? ""} onChange={event => updateRestart({max_retries: event.target.value === "" ? undefined : Number(event.target.value)})}/><small>비워두면 횟수를 제한하지 않습니다.</small></label>
          <label className="editor-field"><span>안정 실행 후 재시작 횟수 초기화 (ms)</span><input type="number" min={0} value={config.restart?.reset_after_ms ?? 60000} onChange={event => updateRestart({reset_after_ms: Number(event.target.value)})}/></label>
        </div>
        <label className="editor-check"><input type="checkbox" checked={config.restart?.jitter ?? true} onChange={event => updateRestart({jitter: event.target.checked})}/>여러 프로세스의 재시작 시각 분산</label>
      </fieldset>
    </details>
  </div>;
}
