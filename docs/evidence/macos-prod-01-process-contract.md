# macOS production process contract — Wave 1 v2 evidence

## 최초 출시 계약

- `core`/`application`은 실제 Direct process spawn·감시·재시작·process-tree 종료와 SystemRegistered 등록·제어를 소유한다.
- HTTP, Tauri invoke, CLI, GUI는 동일 `OperationsFacade` 및 `infra_http::mapping` 결과를 소비한다. route, DTO key, force/cancellation option, CLI exit 의미는 변경하지 않았다.
- 배포 이력이 없는 최초 출시이므로 이전 revision fixture나 first-open/reopen migration은 이 계약의 readiness gate가 아니다.

## v2 대표 검증

| ID | 관찰 | 상태 |
|---|---|---|
| PM-01 | fake ports에서 Direct add/list/get/start/restart/stop/remove, generation advance, running non-force remove conflict, missing get | pass |
| PM-02 | restart backoff 중 stop 뒤 추가 spawn 없음 | pass |
| PM-03 | SystemRegistered register-before-save, start/stop 위임, PID/status, restart noop, convert rollback과 양방향 전환 | pass |
| PM-04 | 실제 ephemeral daemon+CLI에서 config apply, start/logs/restart/stop/remove, process tree 종료, missing process exit `2` | pass |
| PM-05 | GUI HTTP/Tauri adapter의 lifecycle method/path 또는 invoke name, force, argument casing, 반환 mapping 일치 | pass |

## 명령 근거

- `cargo test -p my-supervisor-application --test core_flows representative_process_management` — exit `0`.
- `cargo test -p my-supervisor-application --test core_flows stopping_during_backoff_cancels_restart` — exit `0`.
- `cargo test -p my-supervisor-app-cli --test e2e actual_cli_and_ephemeral_daemon_cover_config_cancel_follow_reconnect_and_exit_codes` — elevated loopback exit `0`.
- `node tests/operations-parity.test.mjs` (`crates/desktop/ui`) — exit `0`; sandbox의 non-fatal Vite HMR bind warning은 결과를 바꾸지 않았다.
- logging 경합 수정 후 `cargo test --workspace` 두 번 — 모두 elevated loopback exit `0`.

## 남은 범위

multi-instance 대규모 rollout, Linux/Windows adapter, installed signed app의 실제 login/reboot 관찰은 이 대표 core 계약으로 통과 처리하지 않는다. 해당 기능은 별도 evidence의 `partial`/미지원 상태를 유지한다.
