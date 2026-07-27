# Wave 7 관찰성 증거

## 구현 범위와 계약

- 추가된 관찰성 도메인/포트는 규칙, 운영 이벤트, 메트릭 표본, 알림 에피소드, 전송 시도와 `(occurred_at_utc, stable_id)` 기반의 경계 포함 페이지를 정의한다.
- SQLite는 규칙과 전이 키가 유일한 운영 이벤트를 별도 additive 테이블에 저장한다. `commit_terminal_run_with_event` 및 `commit_transient_cleanup_terminal`은 각각 terminal run row와 transient outbox 및 해당 operator event를 한 SQLite transaction으로 커밋한다. 기존 process/job/occurrence/run/log 테이블에는 이 변경이 cascade 또는 삭제 권한을 추가하지 않는다.
- HTTP, Tauri, CLI는 새 additive observability route/command로 bounded 조회를 제공한다. 기존 `/api/v1/events`, terminal receipt, CLI follow 및 기존 Tauri 이름은 변경하지 않았다.

## 실행 결과

- `cargo check --workspace` — pass (exit 0).
- `cargo test --workspace` — 2026-07-26 승인된 로컬 환경 최종 재실행은 exit `0`이었다. launchd 3개는 별도 ignored였고 뒤의 대화형 실행에서 각각 통과했다.
- `cargo test -p my-supervisor-app-daemon --test subprocess_recovery_e2e` — 2026-07-26 승인된 로컬 환경에서 exit `0`, 3 passed. terminal outbox crash/replay와 CLI exactly-once 행은 부분 통과 근거로 대체한다.
- `git diff --check` — pass (exit 0).

## 미실행 및 제한

- terminal outbox crash/replay와 CLI exactly-once는 위 2026-07-26 daemon recovery suite로 부분 통과했다. observability reopen/FK, 5s sampling/downsampling, cleanup cursor race, cooldown/ack/recovery, lease reclaim, Notification Center capability, headless fallback, hook timeout/output/env/concurrency/delete fixture는 `unrun`이다.
- 이 문서의 원래 “worker/adapter 미구현” 판정은 현재 source에는 해당 모듈이 존재하므로 stale이다. 다만 source 존재를 동작 통과로 승격하지 않으며, sampler/downsample, alert lifecycle, Notification Center/hook 전체 fixture는 계속 `unrun`이다.
- U09 macOS 관찰과 U13–U14 macOS log E2E는 독립 제한으로 유지한다. 과거 CLI-follow timeout과 workspace-test 종료 상태 미확인은 2026-07-26 재실행으로 대체됐다. 상세 근거는 `.agents/orchestration/20260726-macos-prod-release-readiness-01/phases/interactive-local/results.md`와 `.../automated-gates/results.md`다.
