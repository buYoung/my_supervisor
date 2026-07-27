# macOS Production Operator UI — U20 계약 고정 증거

## 기준선과 범위

- 기준선: U19 handoff `.agents/orchestration/20260725-briefset-macos-prod/implement-u19/work.md`; U01–U19의 완료 source patch 위에서 읽기 전용 대조했다.
- 범위: Wave 9 U20은 `docs/API.md`의 capability/session/json-v2/fixture 계약만 고정한다. shared DTO, HTTP route, Tauri, CLI, TypeScript source 또는 새 test는 만들지 않았다.
- source audit: `crates/infra/http/src/lib.rs` route manifest와 `auth.rs` bearer middleware, `handlers.rs` query bounds, `crates/shared/src/api.rs` DTO, `crates/cli/src/{main.rs,client.rs}`, `crates/desktop/src/main.rs`, `crates/desktop/ui/src/services/{operations-client.ts,invoke-client.ts,http-client.ts,wire-types.ts,wire-mapping.ts}`를 matrix 행별로 대조했다.

## 계약 감사 결과

| 검증 항목 | 관찰 | 상태 |
|---|---|---|
| HTTP auth | `/api/v1/health`를 포함한 router 뒤에 `require_bearer`가 적용되고 unauthorized는 stable `unauthorized` error body를 반환한다. | pass |
| HTTP capability | process, job, logs, daemon/service, events, observability routes가 manifest에 있으며 matrix의 HTTP current 행과 대조했다. | pass |
| DTO/오류/cursor | wire DTO는 shared snake_case source, operation partial outcomes와 logs numeric high-watermark, observability opaque cursor를 확인했다. process/job aggregate paging은 없음으로 기록했다. | pass |
| CLI compatibility | `OutputFormat`은 `table|json`, `print_json`은 pretty JSON, follow는 JSONL이며 current `CliError` exit는 0/1/2/3이다. json-v2는 미구현 additive U21 계약으로만 기록했다. | pass |
| Tauri/TS consumer | desktop은 embedded facade invoke와 debug devBridge이며 `OperationsClient`/HTTP adapter가 무인증 HTTP를 전제한다. installed native proxy/session은 current feature로 표시하지 않고 U21 gap으로 기록했다. | pass |
| session / fixture / scale | cookie/CSRF/Origin/revocation, `50/200` pagination, aggregate concurrency 4, fixture 및 scale input은 U21/U22 execution contract로만 고정했다. | pass (문서 계약) |

## 실행한 검증

- `cargo check --workspace` — repository root에서 pass (exit 0). `my-supervisor-app-desktop`을 포함해 dev profile 완료를 확인했다.
- `git diff --check` — repository root에서 문서 patch 후 pass (exit 0).
- native proxy/session/transport parity, UI typecheck, fixture, scale scenario는 실행하지 않았다. owner는 각각 U21 또는 U22이며, 문서 계약 pass를 runtime pass로 승격하지 않는다.

## U21 인계와 기존 제한

- U21은 shared/application/repository/HTTP/Tauri/CLI/TypeScript에 matrix의 `U21 필수` 행을 additive로 구현하고, bearer를 renderer에 노출하지 않는 native proxy/session 및 json-v2 exit mapping을 검증한다.
- U22는 operator view와 `250 jobs`/`50 instances`/`10,000 entries` scale acceptance를 소유한다.
- 당시 존재했던 CLI follow row-loss와 U09 macOS 관찰, U13–U14, descendant RSS, Notification Center, login/reboot, actual binary switching 제한은 U20의 수정·합격 근거가 아니었다. 최초 출시는 배포된 이전 revision이 없으므로 migration 관찰은 현재 release gate가 아니다.

## U21 실행 기록 (부분 전파)

| 검증 항목 | 관찰 | 상태 |
|---|---|---|
| session bootstrap/logout | `POST /api/v1/session/bootstrap`과 `/logout`이 global auth middleware 뒤에 추가됐다. opaque id는 HttpOnly/Strict/10m `/api/v1` cookie만 사용하며 DTO에는 CSRF nonce와 만료 시각만 있다. bearer generation rotation은 server-side session map을 비운다. | source/compile pass |
| invoke/HTTP client parity | `OperationsClient`, invoke 및 HTTP adapter가 process instance/scale/rolling restart와 job get/update/run get/cancel을 같은 shared wire mapping으로 노출한다. `timed_out`은 UI state union과 existing state-color mapping에서 exhaustive하다. | UI typecheck pass |
| package/workspace checks | core/shared/sqlite/application/http/CLI/desktop package check와 `cargo check --workspace`를 repository root에서 완료했다. | pass |
| workspace tests | 과거 4,104/10,001 row-loss는 2026-07-26 재실행으로 대체됐다. `cargo test --workspace` exit `0`, daemon exact recovery exit `0`, CLI E2E exit `0`; process/run follow 각각 10,001행과 terminal event exactly-once를 검증했다. | pass (2026-07-26 replacement) |

이 기록은 U21 전체 완료나 production-ready 관찰을 뜻하지 않는다. installed-owner native proxy, aggregate/page/partial repository authority, debug fixture control, json-v2/exit implementation, full daemon/config/alert command parity 및 cookie WebView rebootstrap은 후속 구현이 필요하다. U22 화면 workflow/scale acceptance, U23 release gate, U09 macOS 관찰과 기존 외부 제한도 여전히 범위 밖이다.

## U22 재시도 — UI 소비자 기준선

- predecessor revision: `2ede9d6470200ae6877b9f830ca36a66d7a02f89`의 U21 final-gate 기준선에서 page/partial 계약을 소비했다. 이 U22 patch는 아직 commit되지 않은 shared worktree state이므로, 이 evidence의 exact changed-file 목록과 exit-0 command 결과가 U23 전 immutable handoff manifest이며 commit hash로 가장하지 않는다. U20의 `/api/v1`, snake_case, legacy JSON/exit 의미와 cursor shape는 변경하지 않았다.
- process/job page: `ProcessesView`와 `JobsView`는 첫 50개 및 explicit next cursor만 호출한다. refresh failure는 `usePolledResource`의 마지막 성공 data를 보존하며, `partial`과 failed partition은 빈 성공 상태로 변환하지 않고 화면 오류로 남긴다.
- job history: 이전의 모든 job별 `Promise.all` fan-out 및 failure-to-empty 변환을 제거했다. 선택한 job 하나만 50개 bounded history를 요청하며, request failure를 별도 오류로 표시한다. `timed_out`은 defined danger tone으로 렌더링된다.
- process workflow: macOS Direct/SystemRegistered create/convert, bounded instance read, scale, rolling-restart 결과와 per-instance outcome을 `OperationsClient`의 기존 HTTP/invoke path로 소비한다. Linux/Windows 안내, hard-coded 개인 경로, 수동 plist/복사 preview는 제거했다.
- diagnostics: log stream은 existing 1,000-line renderer bound와 tail/follow handoff를 유지하고 cursor-expired, earliest-retained, truncation, dropped state를 화면에 남긴다. disabled `적용` no-op을 제거하고 filter는 즉시 적용된다. daemon panel은 existing global event stream을 bounded 20개로 표시하며 unsupported shutdown control을 제공하지 않는다.

| 검증 | 정확한 명령/관찰 | 상태 |
|---|---|---|
| TypeScript consumer contract | repository root에서 `pnpm --dir crates/desktop/ui typecheck` | pass (exit 0) |
| Rust workspace downstream surface | repository root에서 `cargo check --workspace` | pass (exit 0) |
| production UI build | repository root에서 `pnpm --dir crates/desktop/ui build` | pass (exit 0) |
| patch whitespace | repository root에서 `git diff --check` | pass (exit 0) |
| HTTP devBridge / Tauri invoke interactive flow | 250 jobs, 50 instances, 10,000 entries, injected partition failure, expired cursor, keyboard-only를 포함한 isolated macOS fixture는 이 실행에서 시작하지 않았다. | unrun / mandatory observed evidence missing |

### U23 인계 상태

U22 source 및 static-build 기준선은 위 revision과 명령 결과로 고정한다. 그러나 Wave 9의 mandatory two-transport/macOS scale·partial·cursor·accessibility rows가 `unrun`이므로 이 evidence는 전체 UI scale-ready 근거가 아니다. 기존 CLI follow row-loss는 v2 logging 수정과 workspace 2회 통과로 폐쇄됐지만, U09/U13–U14, descendant RSS, Notification Center, login/reboot, actual binary switching은 unconfirmed로 유지한다.

## 2026-07-26 v2 core 관리 parity

- `node tests/operations-parity.test.mjs`는 HTTP와 Tauri invoke의 process lifecycle method/path 또는 command name, `force`, camelCase argument, 반환 mapping을 대조해 exit `0`이었다.
- omitted Job response의 실제 timezone과 `run_once`가 wire DTO → 공통 mapping → GUI model/view까지 전달됨을 관찰했다. form은 server authority를 유지하며 생략값을 발명하지 않는다.
- `pnpm --dir crates/desktop/ui typecheck`, `build`, `test:follow-events`도 exit `0`이었다.
- 따라서 최초 출시의 대표 CLI/GUI 동등 관리 gate는 `pass`다. 250 jobs/50 instances/10,000 entries, 장애 partition, expired cursor, keyboard-only 전체 matrix는 계속 `partial`이다.
