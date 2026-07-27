# macOS 최초 출시 준비 체크리스트

이 문서는 `20260726-macos-prod-release-readiness-04`의 잔여 보완을 반영한 최초 출시 게이트의 단일 기준이다. 상태는 `pass`, `partial`, `blocked`, `pending-approval`만 사용하며, `partial`, `blocked`, `pending-approval`은 모두 프로덕션 배포를 차단한다.

## 제품 범위

- 신뢰 경계는 실제 프로세스를 spawn·감시·재시작·종료하고 상태와 로그를 보존하는 `core`/`application` 프로세스 엔진이다.
- CLI와 GUI는 동일 `OperationsFacade` 및 wire mapping을 소비하는 동등한 관리 화면이다. Direct와 SystemRegistered의 생명주기 의미를 transport별로 따로 정의하지 않는다.
- 최초 macOS 후보는 `aarch64-apple-darwin`에서 생성한 `arm64` 전용 app과 DMG다. Intel 및 universal artifact는 지원 범위가 아니다.
- 신규 Job에서 timezone, misfire policy, trigger UUID를 생략하면 현재 macOS IANA timezone, `run_once`, 새 UUID를 저장한다. timezone 해석 실패는 UTC 대체 없이 `invalid_config`으로 반환한다.
- 배포 이력이 없는 최초 출시이므로 이전 revision fixture, first-open/reopen migration, 과거 기본값 보존은 출시 조건이 아니다. 저장소의 방어적 read fallback은 새 입력의 기본값 계약이 아니다.
- signing, notarization, stapling, publish, deployment, clean-user 설치, login/reboot 검증은 로컬 source 검증과 별개의 외부 경계다.

## 후보 신원과 provenance

- 후보는 `scripts/release/capture-source-provenance.sh create target/package-security/source-manifest.tsv`가 기록한 실제 build/package 입력의 경로·tracked state·mode·SHA-256과 `Cargo.lock` digest에 결속해야 한다. V2의 `head`는 audit metadata이고 `manifest_sha256` footer가 build-input identity다. 따라서 evidence-only HEAD 변경은 footer가 같은 한 release validity를 무효화하지 않으며 verify는 `head-status=metadata-changed`로 이를 보고한다. 문서·evidence·orchestration 기록은 artifact input이 아니므로 후보 byte 신원에서 제외한다. 다만 같은 로컬 권한으로 ignored `target/`, staged sidecar, manifest와 verifier를 함께 바꿀 수 있으므로 unsigned local verifier는 ignored build artifact가 source에서 유도됐음을 암호학적으로 보장하지 않는다.
- build 후 같은 manifest를 `verify`하여 입력 집합과 bytes가 바뀌지 않았음을 확인한다. 다섯 bundle executable은 대응 release artifact와 byte-for-byte로 같아야 하며 모두 정확히 `arm64`여야 한다.
- desktop Mach-O의 `MSV_SECURITY_CONTRACT_V1` marker는 실제 build가 읽은 CSP, hardened-runtime 입력, entitlement bytes/digest, source manifest digest와 일치해야 한다. credential scan은 같은 tracked+untracked non-ignored 집합을 검사하고 값은 출력하지 않는다.
- release-quality 단계의 manifest digest `1021749781dace176e28f0231974e45bde9b4bbbe94b7cc35e97994119511807`와 DMG SHA-256 `37879935e697c86f94ac1019414039cdc2f9989327c2cccc4974aa3ff26e3993`은 logging 수정 전의 역사적 관측이며 현재 후보가 아니다.
- 이전 v3 후보 manifest SHA-256 `d313503774c70c639efec75b92413c5324468daaf7bad4f87c33fdf6830b2ec1`, DMG SHA-256 `5b7e038bd9c5ec9bd83039ed74f84d722e27457c4f76862c4bf0d282e199215f`, desktop SHA-256 `ee3523415ab4a311bba2e1e6077a2720d521ed75090567bb5f132a8d633a23dc`는 역사적 관측이며 현재 후보가 아니다.
- remediation 전 v4 identity(HEAD `5cc9bebedcac37c98e9e7e311531607031369a44`, provenance digest `6debd1a35f97367d5ef99ac0422bcb651a39565f49f6d087ad25c29875b4f805`, DMG `4cc244b42b42e9087d7a0f4ad6a73c9d4a3a4f4bb7d14c5ebd6fb3bbf4f88f47`)는 V2 provenance source 변경 전의 stale historical observation이며 현재 후보가 아니다.
- 현재 unsigned arm64 후보는 최종 source commit `46eb9c7d3a1ab3956084e89afbb7fa78ca90b783`에서 새로 만든 `MSV_SOURCE_PROVENANCE_V2` manifest `target/package-security/source-manifest.tsv`(142 inputs, V2 build-input footer `fab60fb32dfc20875d49b7833eea29c2fcb3ad37c19ad541125b800a4df7e413`, manifest-file SHA-256 `a467d9ed5acbb39494da32c8e9c1be3c9e9a958a34362cbe3dd1afbef6e57b86`)에 결속한다. app은 `target/package-security/aarch64-apple-darwin/release/bundle/macos/my-supervisor.app`, DMG는 `target/package-security/aarch64-apple-darwin/release/bundle/dmg/my-supervisor_0.1.0_aarch64.dmg`(SHA-256 `3c1c3159f4094e79e8735a8b73499a08f69759617f1405a5bc9ec274d283743d`), desktop executable은 `target/package-security/aarch64-apple-darwin/release/my-supervisor`(SHA-256 `e134ee6f7bdecaab3c45611ceb689190632e8602e1e7cbedb4dcb0fb2e694332`)이다. sidecar SHA-256은 `msv-daemon` `59789e2d93f3f190e97ab1e0543c45f7d3c97b54502b58607778adc4433a3979`, `msv` `2fdc394b19827bbdc53124f9b5e2b083c3c68910ea1cce8a5412c657fb729b1d`, `msv-log-proxy` `e7c199058cdc23f830c4732a905792f3d9b12a3e46bdba0cb0c5bbe1f125d100`, `msv-group-reaper` `3b9dee3746dd809adcaf743016a5f247b09bf1e8cdfc0320d9ba29e08371c7cb`이다. create 직후·build 후·expanded negative matrix 후 provenance verify가 exit `0`이었다.

## 필수 게이트

이번 실행은 package-facing build와 P01–P03을 새로 실행했다. A01–A05의 전체 Rust/UI 검증은 `phases/integrated-verification/results.md`의 이전 통합 실행을 근거로 유지한다. 최종 source `46eb9c7d3a1ab3956084e89afbb7fa78ca90b783`의 Rust/UI bytes는 그 검증 이후 변경되지 않았으며, 이번에 실행하지 않은 전체 Rust/UI gate를 재통과했다고 주장하지 않는다.

| ID | 상태 | 게이트 | 현재 근거 또는 충족 기준 |
|---|---|---|---|
| A01 | pass | Rust 형식·정적 검사 | v4 현재 source에서 `cargo fmt --all -- --check`, `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`가 모두 exit `0` |
| A02 | pass | core와 대표 운영 계약 | 신규 Job defaults, Direct/SystemRegistered 대표 lifecycle, 실제 daemon+CLI E2E, recovery E2E가 exit `0`; v4 현재 source의 유효한 local loopback 권한 `cargo test --workspace` 2회가 모두 exit `0` |
| A03 | pass | CLI/GUI 동등 관리 화면 | v4 UI typecheck/build/event-follow와 HTTP/Tauri operations parity가 exit `0`; 실제 GUI 삭제 handler의 non-force, 확인된 force, 취소된 force 및 HTTP/Tauri boolean을 관찰 |
| A04 | pass | 시간대 설정 오류 | `SchedulerError::InvalidTimezone`이 UTC fallback 없이 stable `invalid_config`/HTTP 400으로 전파되도록 수정했고 focused defaults/config mapping 검증이 exit `0` |
| A05 | pass | recovery root 소유권 정리 | recovery E2E 3개가 exact root와 UUID marker를 출력하고 exit `0`; 일치하는 non-symlink marker를 가진 exact root만 명시 cleanup과 `Drop` fallback에서 삭제 |
| P01 | pass | 로컬 arm64 후보 일관성 | fresh V2 manifest에서 `RUSTFLAGS='-D warnings'` explicit `aarch64-apple-darwin` sidecars, prepare, unsigned Tauri app/DMG가 생성됐고 build 전후 provenance verify가 exit `0` |
| P02 | pass | 로컬 후보 내용·경로·정리 | supplied app과 read-only DMG의 다섯 executable이 대응 release artifact와 byte-equal이고 모두 exact `arm64`; marker, credential scan 0, unsigned entitlement 관찰, DMG checksum, verifier 독립 source/app/mounted-DMG 비교와 exact mount cleanup이 통과 |
| P03 | pass | 로컬 tamper 거부 | replaced desktop, marker corruption, credential canary, x86_64, wrong arbitrary-root DMG, tampered main/sidecar DMG, external app symlink, attach-parser-failure를 모두 거부했고 `negative-self-checks=passed`/exit `0`; 이 범위는 local path/cleanup 검사뿐 |
| C01 | partial | GitHub-hosted CI | immutable revision checkout과 clean hosted build, source revision·artifact digest를 결속한 attestation 성공 실행이 아직 관찰되지 않음 |
| E01 | pending-approval | Developer ID signing/notarization/stapling | 별도 승인, Apple credential, network 및 현재 source의 실제 signed/stapled candidate가 필요 |
| E02 | blocked | 외부 설치 검증 | clean macOS user/VM에서 DMG 설치·first launch·CLI/GUI 관리·login/reboot·uninstall/data preservation 관찰이 필요 |

상태 합계는 총 11개 중 `pass` 8개, `partial` 1개, `pending-approval` 1개, `blocked` 1개다. 남은 게이트 ID는 `C01`, `E01`, `E02`다.

같은 경로의 target swap은 정상 실패에도 사용자 build state를 훼손할 수 있고 비정상 종료 복구를 보장할 수 없어 채택하지 않았다. `e9702b0`의 isolated rebuild 시도와 그 revert는 이 결론을 뒷받침하는 역사적 engineering note일 뿐 현재 release gate가 아니다.

## 비핵심 확장 매트릭스

다음 항목은 관측되지 않은 범위를 통과로 승격하지 않는다. 다만 PM2형 최초 출시의 core/CLI/GUI 배포 목적보다 우선하지 않으며, 해당 기능을 출시 지원 범위로 주장할 때 별도 gate가 된다.

- scheduler의 전체 DST/catch-up/retry/queue/dependency 조합: `partial`
- sustained rollover/retention, SystemRegistered 외부 로그 ingestion, 모든 cursor-expiry transport 조합: `partial`
- 250 jobs/50 instances/10,000 entries, 장애 partition, keyboard-only를 포함한 전체 GUI scale·accessibility 매트릭스: `partial`
- Linux/Windows, Rules, Notification Center, 광범위 metrics/alert/hook 매트릭스: `blocked` 또는 현재 미지원

## 남은 작업과 소유자

1. 저장소 CI 소유자 — `C01`의 GitHub-hosted workflow를 실행해 representative core/CLI/GUI와 package job의 성공 URL/run ID를 기록한다.
2. Apple release 소유자 — 사용자 승인 후 `E01`에서 현재 후보를 Developer ID로 서명하고 notarize/staple한 뒤 signed verification 결과를 기록한다.
3. macOS QA 소유자 — signed/notarized 후보가 준비되면 `E02`에서 clean-user/VM 설치, CLI/GUI의 동일 process management, login/reboot 복구, uninstall/data preservation을 관찰한다.

## 출시 결정

core process engine과 대표 CLI/GUI 운영 계약, 그리고 현재 unsigned arm64 후보의 로컬 consistency/tamper/path/cleanup 게이트(P01–P03)는 통과했다. ignored build artifact가 source-derived임을 보장하는 것은 로컬 unsigned verifier의 역할이 아니며, C01의 clean hosted CI immutable revision+artifact attestation 및 E01의 Developer ID signing/notarization/stapling에서 완료되어야 한다. 따라서 production deployment는 준비되지 않았다. signing, notarization, publication 또는 deployment가 수행됐다는 주장은 하지 않는다.
