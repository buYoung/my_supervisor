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

- 후보는 `scripts/release/capture-source-provenance.sh create target/package-security/source-manifest.tsv`가 기록한 실제 build/package 입력의 경로·mode·SHA-256, HEAD, `Cargo.lock` digest에 결속해야 한다. 문서·evidence·orchestration 기록은 artifact input이 아니므로 후보 byte 신원에서 제외한다.
- build 후 같은 manifest를 `verify`하여 입력 집합과 bytes가 바뀌지 않았음을 확인한다. 다섯 bundle executable은 대응 release artifact와 byte-for-byte로 같아야 하며 모두 정확히 `arm64`여야 한다.
- desktop Mach-O의 `MSV_SECURITY_CONTRACT_V1` marker는 실제 build가 읽은 CSP, hardened-runtime 입력, entitlement bytes/digest, source manifest digest와 일치해야 한다. credential scan은 같은 tracked+untracked non-ignored 집합을 검사하고 값은 출력하지 않는다.
- release-quality 단계의 manifest digest `1021749781dace176e28f0231974e45bde9b4bbbe94b7cc35e97994119511807`와 DMG SHA-256 `37879935e697c86f94ac1019414039cdc2f9989327c2cccc4974aa3ff26e3993`은 logging 수정 전의 역사적 관측이며 현재 후보가 아니다.
- 이전 v3 후보 manifest SHA-256 `d313503774c70c639efec75b92413c5324468daaf7bad4f87c33fdf6830b2ec1`, DMG SHA-256 `5b7e038bd9c5ec9bd83039ed74f84d722e27457c4f76862c4bf0d282e199215f`, desktop SHA-256 `ee3523415ab4a311bba2e1e6077a2720d521ed75090567bb5f132a8d633a23dc`는 역사적 관측이며 현재 후보가 아니다.
- 현재 v4 unsigned arm64 후보는 HEAD `5cc9bebedcac37c98e9e7e311531607031369a44`의 fresh provenance manifest `target/package-security/source-manifest.tsv`(142 inputs, provenance digest `6debd1a35f97367d5ef99ac0422bcb651a39565f49f6d087ad25c29875b4f805`, manifest-file SHA-256 `48ef39621ace75c177e070b20906f3bc12d8232358dfa6653e8309c6fa54e320`)에 결속한다. app은 `target/package-security/aarch64-apple-darwin/release/bundle/macos/my-supervisor.app`, DMG는 `target/package-security/aarch64-apple-darwin/release/bundle/dmg/my-supervisor_0.1.0_aarch64.dmg`(SHA-256 `4cc244b42b42e9087d7a0f4ad6a73c9d4a3a4f4bb7d14c5ebd6fb3bbf4f88f47`), desktop executable은 `target/package-security/aarch64-apple-darwin/release/my-supervisor`(SHA-256 `dddc1f1270402e31314ca8277621ae909fec25a75ba462706782771cf2fc8dcf`)이다. build 전후 및 negative matrix 후 provenance verify가 모두 통과했다.

## 필수 게이트

| ID | 상태 | 게이트 | 현재 근거 또는 충족 기준 |
|---|---|---|---|
| A01 | pass | Rust 형식·정적 검사 | v4 현재 source에서 `cargo fmt --all -- --check`, `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`가 모두 exit `0` |
| A02 | pass | core와 대표 운영 계약 | 신규 Job defaults, Direct/SystemRegistered 대표 lifecycle, 실제 daemon+CLI E2E, recovery E2E가 exit `0`; v4 현재 source의 유효한 local loopback 권한 `cargo test --workspace` 2회가 모두 exit `0` |
| A03 | pass | CLI/GUI 동등 관리 화면 | v4 UI typecheck/build/event-follow와 HTTP/Tauri operations parity가 exit `0`; 실제 GUI 삭제 handler의 non-force, 확인된 force, 취소된 force 및 HTTP/Tauri boolean을 관찰 |
| A04 | pass | 시간대 설정 오류 | `SchedulerError::InvalidTimezone`이 UTC fallback 없이 stable `invalid_config`/HTTP 400으로 전파되도록 수정했고 focused defaults/config mapping 검증이 exit `0` |
| A05 | pass | recovery root 소유권 정리 | recovery E2E 3개가 exact root와 UUID marker를 출력하고 exit `0`; 일치하는 non-symlink marker를 가진 exact root만 명시 cleanup과 `Drop` fallback에서 삭제 |
| P01 | pass | 현재 source의 arm64 warning-free 후보 | fresh manifest에서 `RUSTFLAGS='-D warnings'` arm64 sidecars, explicit `aarch64-apple-darwin` unsigned app/DMG가 생성됐고 build 전후 provenance verify가 exit `0` |
| P02 | pass | 현재 후보 내용·보안 경계 | supplied app과 read-only DMG의 다섯 executable이 대응 release artifact와 byte-equal이고 모두 exact `arm64`; marker, credential scan 0, unsigned entitlement 관찰, pre/post-negative provenance, DMG checksum이 통과 |
| P03 | pass | 현재 후보 negative self-check | replaced desktop, marker corruption, credential canary, x86_64, wrong arbitrary-root DMG, tampered main/sidecar DMG, external app symlink, attach-parser-failure exact cleanup을 모두 거부 |
| C01 | partial | GitHub-hosted CI | 대표 core/CLI/GUI 및 arm64 provenance package workflow는 구성됨. GitHub-hosted 성공 실행은 관찰되지 않음 |
| E01 | pending-approval | signing/notarization | 별도 승인, Apple credential, network 및 현재 source의 실제 signed candidate가 필요 |
| E02 | blocked | 외부 설치 검증 | clean macOS user/VM에서 DMG 설치·first launch·CLI/GUI 관리·login/reboot·uninstall/data preservation 관찰이 필요 |

상태 합계는 총 11개 중 `pass` 8개, `partial` 1개, `pending-approval` 1개, `blocked` 1개다. 남은 게이트 ID는 `C01`, `E01`, `E02`다.

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

core process engine과 대표 CLI/GUI 운영 계약, 그리고 현재 v4 unsigned arm64 package의 로컬 자동 게이트(P01–P03)는 통과했다. 그러나 GitHub-hosted CI, Developer ID signing/notarization/stapling, clean-user/VM 설치·login/reboot·uninstall 검증은 아직 관찰되지 않았으므로 production deployment는 준비되지 않았다. signing, notarization, publication 또는 deployment가 수행됐다는 주장은 하지 않는다.
