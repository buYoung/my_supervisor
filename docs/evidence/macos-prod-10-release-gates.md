# macOS release gates — Wave 10 역사적 evidence

- 원 실행 ID: `20260726-macos-prod-release-readiness-02`
- 관찰일: `2026-07-26`
- 제품 결정: 실제 프로세스를 관리하는 core engine이 신뢰 경계이며 CLI와 GUI는 동등한 관리 화면이다. 최초 artifact는 `arm64` 전용이다.
- source HEAD: `2ede9d6470200ae6877b9f830ca36a66d7a02f89`; 작업 트리 변경은 HEAD만으로 재현할 수 없으므로 content-addressed manifest가 후보 신원의 필수 요소다.
- 최초 출시에는 배포된 이전 revision이 없다. 이전 binary/database first-open migration과 과거 기본값 보존은 이 evidence의 gate가 아니다.

## v2 자동 명령 증거

세부 중간 실패와 수정은 [process-contract](../../.agents/orchestration/20260726-macos-prod-release-readiness-02/phases/process-contract/results.md), [package-hardening](../../.agents/orchestration/20260726-macos-prod-release-readiness-02/phases/package-hardening/results.md), [release-quality](../../.agents/orchestration/20260726-macos-prod-release-readiness-02/phases/release-quality/results.md), [logging-flake-remediation](../../.agents/orchestration/20260726-macos-prod-release-readiness-02/phases/logging-flake-remediation/results.md)에 보존한다.

| 명령/검사 | exit | 관찰/판정 |
|---|---:|---|
| `cargo fmt --all -- --check` | 0 | logging 수정 후 전체 Rust format 통과 |
| `cargo check --workspace` | 0 | logging 수정 후 workspace type-check 통과 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | release warning 0 포함 정적 검사 통과 |
| focused Job defaults/config mapping/process tests | 0 | 현재 IANA timezone, `run_once`, non-nil UUID 및 representative lifecycle 통과 |
| CLI exact E2E | 0 | 실제 ephemeral daemon에서 config apply, process lifecycle, logs/follow, 오류 exit 계약 통과 |
| daemon recovery E2E `--nocapture` | 0 | 3개 exact root/UUID marker identity와 cleanup 통과 |
| UI typecheck/build/event-follow/operations parity | 0 | HTTP/Tauri lifecycle shape 및 Job metadata 소비자 전달 통과 |
| `cargo test --workspace` after logging fix | 0, 0 | elevated loopback 전체 실행 2회 모두 clean |
| detached journal exact after fix | 10/10 exit 0 | sequence/cursor/terminal race 반복 통과 |
| durable journal current exact stress after fix | 10/10 exit 0 | 10개 동시 process 반복 통과 |

비상승 기록: non-elevated CLI/workspace 실행의 loopback `Operation not permitted`는 sandbox 제한이다. `git diff --check`는 이 단계가 수정하지 않은 `AGENTS.md:39` whitespace 때문에 exit `2`이며 통과로 기록하지 않는다.

## 신규 Job 계약

| 경계 | 관찰 | 상태 |
|---|---|---|
| TOML bootstrap | 생략 timezone을 runtime IANA zone, misfire를 `run_once`, trigger ID를 새 UUID로 변환 | pass |
| HTTP direct/config apply | 두 경로가 같은 `job_config_to_job`을 사용하고 exact defaults contract test 통과 | pass |
| CLI config apply/query | persisted non-empty timezone, `run_once`, non-empty UUID 관찰 | pass |
| GUI HTTP/Tauri | form은 생략값을 발명하지 않고 server response의 timezone/misfire를 공통 mapping으로 최종 model/view에 전달 | pass |
| timezone 해석 실패 | UTC fallback 없이 stable `invalid_config`/HTTP 400으로 전파 | pass |

명시적으로 전달한 timezone, misfire policy, trigger UUID는 기존 conversion branch가 보존한다. 저장소의 방어적 read fallback은 신규 입력의 기본값 또는 출시 호환 gate가 아니다.

## 대표 core/CLI/GUI 관리 매트릭스

| ID | 관찰 | 상태 |
|---|---|---|
| PM-01 Direct | add/list/get/start/restart/graceful stop/remove, generation advance, running non-force remove conflict, missing get | pass |
| PM-02 restart cancellation | backoff 중 stop이 예약 restart를 취소하고 추가 spawn 없음 | pass |
| PM-03 SystemRegistered | register-before-save, start/stop 위임, PID/status, restart noop, 실패 convert rollback, Direct↔SystemRegistered 전환 | pass |
| PM-04 CLI+daemon | isolated root에서 config, start/logs/restart/stop/remove와 missing process exit `2`; 실제 process tree 종료 | pass |
| PM-05 GUI parity | HTTP path/method/force와 Tauri command/camelCase args, 반환 mapping 및 Job metadata가 동일 | pass |

public `/api/v1` route, DTO key/shape, CLI command 및 기존 exit 의미는 바뀌지 않았다. CLI와 GUI는 별도 lifecycle rule을 추가하지 않고 같은 facade 결과를 전달한다.

## 패키지·보안 역사적 관측과 현재성

여기의 unsigned arm64 관측은 당시 실제 build/package 입력 142개 manifest SHA-256 `d21420b31706d3fceb0cc9fffd21eb29a0f5cd2444d4f5fb70b745477d441760`에 대한 역사적 기록이다. 문서·evidence·orchestration 기록은 artifact input이 아니므로 후보 byte 신원에서 제외한다. 현재 후보 신원은 아래 final-evidence V2 section만 사용한다.

app은 `target/package-security/aarch64-apple-darwin/release/bundle/macos/my-supervisor.app`이고 desktop SHA-256은 `50d424c9e25ea443d9a6eefa26abe08ae50fc8a39640d0111e77d9eb23ec51c4`이다. DMG는 `target/package-security/aarch64-apple-darwin/release/bundle/dmg/my-supervisor_0.1.0_aarch64.dmg`, SHA-256 `252657541aad899a123e416d2f08c4eae9e2e41329625bdd9403930ed14810ca`이며 `hdiutil verify`가 통과했다.

다섯 executable은 source/app/readonly-mounted DMG에서 byte-for-byte 동일하고 exact `arm64`였다: `my-supervisor` `50d424c9e25ea443d9a6eefa26abe08ae50fc8a39640d0111e77d9eb23ec51c4`, `msv-daemon` `59789e2d93f3f190e97ab1e0543c45f7d3c97b54502b58607778adc4433a3979`, `msv` `2fdc394b19827bbdc53124f9b5e2b083c3c68910ea1cce8a5412c657fb729b1d`, `msv-log-proxy` `e7c199058cdc23f830c4732a905792f3d9b12a3e46bdba0cb0c5bbe1f125d100`, `msv-group-reaper` `3b9dee3746dd809adcaf743016a5f247b09bf1e8cdfc0320d9ba29e08371c7cb`.

candidate 내부 `MSV_SECURITY_CONTRACT_V1` marker, credential scan 0, unsigned codesign 관찰 `embedded-entitlements=unsigned-empty-output`이 통과했다. 이 unsigned 관찰은 signed hardened-runtime enforcement를 주장하지 않는다. desktop executable replacement, marker corruption, untracked credential canary, x86_64 input의 네 negative self-check는 각각 거부됐고 outer verifier는 exit `0`이었다. credential 값은 출력하지 않았다. logging 수정 전 manifest `1021749781dace176e28f0231974e45bde9b4bbbe94b7cc35e97994119511807` 및 DMG SHA-256 `37879935e697c86f94ac1019414039cdc2f9989327c2cccc4974aa3ff26e3993`은 역사적 관측으로만 유지한다.

`20260726-macos-prod-release-readiness-03`의 v3 manifest `d313503774c70c639efec75b92413c5324468daaf7bad4f87c33fdf6830b2ec1`, DMG `5b7e038bd9c5ec9bd83039ed74f84d722e27457c4f76862c4bf0d282e199215f`, desktop `ee3523415ab4a311bba2e1e6077a2720d521ed75090567bb5f132a8d633a23dc`도 `20260726-macos-prod-release-readiness-04`의 GUI 삭제 및 package verifier build-input 변경으로 stale이며 현재 후보나 release gate 통과로 사용하지 않는다.

## final-evidence V2 unsigned arm64 후보

- remediation 전 v4 identity(HEAD `5cc9bebedcac37c98e9e7e311531607031369a44`, provenance digest `6debd1a35f97367d5ef99ac0422bcb651a39565f49f6d087ad25c29875b4f805`, DMG `4cc244b42b42e9087d7a0f4ad6a73c9d4a3a4f4bb7d14c5ebd6fb3bbf4f88f47`)는 provenance V2 source 변경 이전의 stale historical observation이다. 현재 후보는 clean source HEAD `4a63a0a237a8a2a344e01dc704093b7b22161619`에서 새로 생성했다.
- fresh `target/package-security/source-manifest.tsv`는 `MSV_SOURCE_PROVENANCE_V2`, 142 inputs, `Cargo.lock` SHA-256 `37cef515c79c08ee36881097883d19f7fd29773f98dfe18176f89c8bc72a97c5`, build-input footer `fab60fb32dfc20875d49b7833eea29c2fcb3ad37c19ad541125b800a4df7e413`, manifest-file SHA-256 `cef0ecf39bb38c3acf79d6184d9a762a9caeb13852999753c816646e3590fc8c`을 기록한다. V2의 `head`는 audit metadata이고 footer가 build-input identity다. create 직후, app/DMG build 후, expanded negative matrix 후 `verify`는 모두 exit `0` 및 `head-status=unchanged`였다.
- `RUSTFLAGS='-D warnings'` explicit `aarch64-apple-darwin` sidecars와 prepare 뒤 unsigned Tauri app/DMG가 fresh build됐다. app은 `target/package-security/aarch64-apple-darwin/release/bundle/macos/my-supervisor.app`, DMG는 `target/package-security/aarch64-apple-darwin/release/bundle/dmg/my-supervisor_0.1.0_aarch64.dmg`(SHA-256 `8a1b4cb2c6992b2956f48bbbb27ecd5a3a314bfd6b898314856e8ff0342a8e96`), desktop executable은 `target/package-security/aarch64-apple-darwin/release/my-supervisor`(SHA-256 `e134ee6f7bdecaab3c45611ceb689190632e8602e1e7cbedb4dcb0fb2e694332`)이다.
- normal verifier는 supplied app과 read-only DMG의 다섯 executable이 대응 release artifact와 byte-equal이고 exact `arm64`임을 확인했다: `my-supervisor` `e134ee6f7bdecaab3c45611ceb689190632e8602e1e7cbedb4dcb0fb2e694332`, `msv-daemon` `59789e2d93f3f190e97ab1e0543c45f7d3c97b54502b58607778adc4433a3979`, `msv` `2fdc394b19827bbdc53124f9b5e2b083c3c68910ea1cce8a5412c657fb729b1d`, `msv-log-proxy` `e7c199058cdc23f830c4732a905792f3d9b12a3e46bdba0cb0c5bbe1f125d100`, `msv-group-reaper` `3b9dee3746dd809adcaf743016a5f247b09bf1e8cdfc0320d9ba29e08371c7cb`. `hdiutil verify`는 VALID, credential scan은 0, unsigned codesign 관찰은 `embedded-entitlements=unsigned-empty-output`이었다.
- verifier와 독립된 read-only comparison도 source/app/mounted-DMG의 같은 다섯 binary byte equality, exact `arm64`, 위 SHA-256을 재확인했다. normal/negative verifier와 independent comparison은 생성한 exact device만 detach하고 mount directory removal을 관찰했다. post-negative `hdiutil info`에는 `my-supervisor`, `package-security`, 독립 mount 항목이 없었다.
- expanded negative matrix는 replaced desktop, marker corruption, untracked credential canary, x86_64 input, checksum-valid arbitrary-root DMG, changed main/sidecar DMG, top-level `my-supervisor.app` external symlink, attach 후 parser-failure injection을 모두 거부했고 `negative-self-checks=passed` 및 exit `0`으로 종료했다.

## logging 경합 수정 증거

- detached reader는 파일 행과 manifest를 서로 다른 시점에 관찰해 `102 rows + high_watermark 101`을 반환할 수 있었다. 수정 후 실제 관찰한 최대 sequence만 watermark로 반환하고 rollover snapshot을 bounded cooperative retry로 일치시킨다.
- durable journal writer는 append 성공 전에 Tokio file buffer `flush`를 기다리지 않아 high-watermark보다 짧은 JSONL prefix가 보일 수 있었다. 수정 후 complete JSONL 행마다 `flush().await` 뒤에만 watermark/ring/broadcast를 전진시킨다. 기존 64행 `sync_data` cadence는 유지한다.
- 수정 전 detached exact는 8번째 반복에서 재현됐다. 수정 후 detached 10/10, durable concurrent exact 10/10, infra logging package, macOS platform package, workspace 전체 2회가 모두 exit `0`이었다.

## recovery root identity

recovery E2E는 세 scenario의 exact temporary root와 별도 UUID marker를 `--nocapture`로 기록했다. 명시 cleanup과 panic-safe `Drop` cleanup은 요청한 exact root의 non-symlink marker file이 같은 UUID일 때만 `remove_dir_all`을 호출한다. prefix가 비슷한 foreign root는 삭제 대상이 아니다.

## 게이트 산술과 외부 경계

| 상태 | 수 | ID |
|---|---:|---|
| pass | 8 | A01, A02, A03, A04, A05, P01, P02, P03 |
| partial | 1 | C01 |
| pending-approval | 1 | E01 |
| blocked | 1 | E02 |

- `C01`: 저장소 CI 소유. workflow 구성은 완료됐지만 GitHub-hosted 성공 run은 관찰되지 않았다.
- `E01`: Apple release 소유. signing/notarization/stapling은 credential, network와 별도 승인이 필요하다.
- `E02`: macOS QA 소유. clean-user/VM 설치, CLI/GUI 동등 관리, login/reboot, uninstall/data preservation은 미실행이다.
- 전체 DST/retry/queue/dependency, sustained rollover/retention, SystemRegistered 외부 ingestion, 광범위 metrics/alerts, Linux/Windows, 250/50/10,000 GUI scale·accessibility 매트릭스는 관찰 범위만 `partial` 또는 미지원으로 유지한다.

## 출시 결정

core process engine과 대표 CLI/GUI 운영 계약의 역사적 로컬 source gate 및 현재 v4 unsigned arm64 후보의 P01–P03 로컬 자동 게이트는 통과했다. 그러나 hosted CI, Developer ID signing/notarization/stapling 및 clean-user/VM 설치·login/reboot·uninstall 증거는 준비되지 않았으므로 production deployment는 준비되지 않았다. publish, upload, signing, notarization, stapling 또는 deployment를 수행했다는 주장은 하지 않는다.
