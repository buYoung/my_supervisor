# 데스크톱 UX 네이티브 테스트 기반

`prerequisite_ready: true` — 테스트 기반의 필수 검증을 완료했다. 제품 결함 수정과 OS 관찰은 아래 소유 브리프로 넘긴다.

## 기준과 범위

- 기준 커밋: `748537710413f224be9f25a65d114426b33c0770`, `feat/desktop-parity`
- 작업 환경: macOS 15.7.4 (24G517), Node 24.14.0, pnpm 10.11.0, Cargo 1.95.0
- 기존 `.codemap` 변경 및 사용자 브리프 파일을 보존한다. 커밋·배포는 수행하지 않는다.
- 검사 대상은 실제 React 앱과 기존 Tauri invoke 핸들러다. 테스트 전용 `desktop-e2e` 기능은 디버그 빌드에만 허용한다.
- 전체 시나리오와 소유 브리프는 [scenario-ledger.json](scenario-ledger.json)에 등록했다. 미실행 항목은 통과로 취급하지 않는다.

## 구현 계약

- Rust `tauri-plugin-wdio` 및 `tauri-plugin-wdio-webdriver` 1.5.1, JS `@wdio/tauri-service` 및 `@wdio/tauri-plugin` 1.5.1을 고정한다. WDIO 실행기·Mocha는 9.31.9다.
- 실제 설치된 플러그인은 Cargo의 `custom-protocol` 기능만 제공한다. 존재하지 않는 `devtools` 기능을 가정하지 않는다.
- `tauri.e2e.conf.json`만 전역 Tauri API, WDIO 실행 권한 및 `unsafe-eval`을 허용한다. 일반 `tauri.conf.json`과 기본 capability는 수정하지 않았다.
- `MSV_DESKTOP_E2E=1`, 정규화된 임시 루트, 소유 마커와 유효한 포트를 모두 검사한 뒤 앱을 시작한다. 설정·SQLite·로그·헬퍼 출력은 이 루트 안에 둔다.
- E2E 빌드에서는 devBridge를 제외하여 독립된 `data_dir()/devbridge.json` 쓰기를 방지한다. 일반 디버그 빌드도 `MSV_DEVBRIDGE_PORT`를 명시할 때만 devBridge를 시작하며, 릴리스에서는 컴파일하지 않는다.
- 테스트용 JS 모듈만 `__MSV_E2E__`를 노출한다. 실제 `getOperationsClient()`와 동일한 인스턴스를 사용한다.
- 설치된 WDIO 모킹은 전역 `core.invoke`를 감싸므로, N-M 테스트 동안에만 테스트 빌드의 ESM invoke 내보내기를 모킹 레지스트리에 연결한다. Tauri의 읽기 전용 내부 속성은 수정하지 않는다. `restoreAllMocks`와 원래 함수 복원을 모두 수행하고 N-R 읽기로 확인한다.
- 모든 공개 명령 이름, DTO, ID, expected config/revision 및 릴리스 embedded-owner 거부 정책은 유지한다.

## 실행 명령

작업 디렉터리: `/Users/buyonglee/Documents/work/private/my_supervisor`

```sh
pnpm --dir crates/desktop/ui typecheck
cargo check --workspace
pnpm --dir crates/desktop/ui test:native
```

네이티브 실행기는 기존 `scripts/build-local.sh cli debug`로 실제 헬퍼를 만든 다음 `cargo tauri build --debug --no-sign --bundles app --features desktop-e2e --config tauri.e2e.conf.json`을 `crates/desktop`에서 실행한다. 번들 프런트엔드와 별도 `target/desktop-e2e`를 사용한다. 실행 후 출력하는 임시 루트에 명령 명세, WebView 식별, 스크린샷, 로그, 결과와 정리 상태를 남긴다.

## 확인된 원래 동작

- `command-model.ts`의 `rowsToArguments`는 입력 순서 대신 scope 순으로 재배치한다. 자식 02의 CMD 시나리오로 귀속한다.
- `CommandEditor`는 mount 중 도움말 실행을 요청하고, 구문 오류를 외부 저장 유효성에 전달하지 않는다. 자식 02가 소유한다.
- `JobEditor`의 의존성 입력은 매 입력마다 쉼표 분리·공백 제거를 한다. 자식 03이 소유한다.
- 일반 스타일과 `process-controls.css`의 선언 우선순위는 실제 네이티브 계산값으로 확인하고 자식 09가 소유한다.

## 실행 결과와 한계

- 초기 `pnpm --dir crates/desktop/ui typecheck`: 종료 코드 0
- 초기 `cargo check --workspace`: 종료 코드 0. 전역 Cargo 캐시 파일 삭제 권한 경고가 있었으며 컴파일은 성공했다.
- 일반 UI `build`: 종료 코드 0. 산출물에서 `__MSV_E2E__`, `wdioTauri`, `__wdio_mocks__`, `native-harness` 문자열이 모두 없음을 확인했다.
- 기존 parity 및 follow-events: 종료 코드 0. 각각 Vite HMR의 `listen EPERM: operation not permitted 0.0.0.0:24678` 경고가 있었다. 이 검증은 R 모드이며 실제 소켓 또는 네이티브 통합 통과로 해석하지 않는다.
- 첫 네이티브 빌드 중 WebView 저장소 격리를 추가하기 위해 중단했다. 외부 실행기 종료 시 Cargo 자식 그룹이 잠시 남아 있는 것을 관찰해 IPC 수명 감시기를 추가했다. 네이티브 앱은 해당 실행에서 시작하지 않았다.
- 테스트 창은 `incognito: true`로 설정하여 기존 앱의 WebView 저장소를 사용하지 않는다.
- 네이티브 `main` 창, 실제 invoke 저장·재조회, 모킹 오류 표시 및 모킹 복원 뒤 실제 값 조회를 확인했다. 최초 직접 내부 속성 교체는 읽기 전용 속성 오류로 실패했으며, E2E 전용 모듈 별칭으로 수정했다.
- 정상·의도적인 시작 실패·테스트 실패·중단에서 앱과 포트가 종료됨을 확인했다. 중단 때 남는 전용 프로세스는 실행기에서 명시적으로 정리했다.
- 최신 전체 빌드와 기본 재현 검증은 진행 중이다. 아직 인수 완료로 판단하지 않는다.
- 한글 IME, 네이티브 파일 선택·취소, 클립보드 및 트레이 관찰은 아직 미실행이다. WebDriver 문자열 입력을 OS IME 증거로 대체하지 않는다.
- 디버그 앱 결과를 서명된 설치형 릴리스의 실행 가능성으로 해석하지 않는다.

## 출처

[공식 WDIO Tauri 설정](https://webdriver.io/docs/desktop-testing/tauri/configuration), [플러그인 설정](https://webdriver.io/docs/desktop-testing/tauri/plugin-setup), [모킹 API](https://webdriver.io/docs/desktop-testing/tauri/api)를 2026-10-27 확인했다. 문서와 설치 소스가 다를 때 설치한 1.5.1의 실제 코드를 기준으로 구성했다.

## 인수 결과

- HARNESS-01: N-R 통과. `main`, `tauri://localhost`, WebKit 605.1.15에서 실제 invoke로 프로세스 정의와 빈 인수·유니코드·쉼표를 저장하고 재조회했다.
- HARNESS-02: N-M → N-R 통과. `cmd_list_processes` 거부가 실제 UI에 표시됐고, `restoreAllMocks` 뒤 모듈의 invoke가 원래 함수로 복원된 것을 확인했다. 이후 같은 어댑터로 저장된 값 조회가 성공했다.
- HARNESS-03: 정상, 시작 실패, 의도적 assertion 실패, SIGINT 중단의 포트·앱·전용 실행 프로세스 정리 통과. 중단 경로에서 종료되지 않은 전용 자식은 실행기가 정리한다.
- HARNESS-04: 전체 65개 ID 등록. 초기 제품 결함의 구체적인 기대/관찰값은 [baseline.json](01-artifacts/native/baseline.json)에 있다. 입력·포커스 자동화는 DOM 기반이며 OS IME 관찰로 해석하지 않는다.
- HARNESS-05: [일반 빌드 검사](01-artifacts/normal-build-audit.json)에서 WDIO Rust 의존성 활성화와 프런트엔드 훅이 없고 일반 CSP가 그대로임을 확인했다. 기본 빌드에서 devBridge는 명시적으로 포트를 지정해야 열린다.

| 경로 | 실행 명령 (저장소 루트) | 결과 |
| --- | --- | --- |
| 기본 | `pnpm --dir crates/desktop/ui test:native --no-build` | 3개 통과, 정리 통과 |
| 시작 실패 | `pnpm --dir crates/desktop/ui test:native --no-build --probe=startup` | 의도한 code=101 확인, 정리 통과 |
| 테스트 실패 | `pnpm --dir crates/desktop/ui test:native --no-build --probe=failure` | 의도한 assertion 실패 확인, 정리 통과 |
| 중단 | `pnpm --dir crates/desktop/ui test:native --no-build --probe=interrupt` | 준비 마커 뒤 SIGINT 확인, 정리 통과 |

빌드 명령은 위 실행 명령에서 `--no-build`를 빼면 된다. [실제 앱 식별](01-artifacts/native/identity.json), [실행 명세](01-artifacts/native/manifest.json), [시나리오 결과](01-artifacts/native/results.jsonl), [테스트 소스 해시](01-artifacts/test-source-sha256.json), 각 하위 폴더의 `cleanup.json`으로 결과를 추적할 수 있다. 앱 바이너리는 마지막 전체 빌드의 제품 소스이며, 그 뒤 수정한 재현 선택자·실행 스크립트는 별도 해시로 기록했다.

## 다음 브리프에 전달할 내용

- 02: 비인터프리터 `/usr/bin/printf`의 `run start -- --port 3000`에서 마지막 값만 바꾸는 실제 화면 사례. 자동 도움말 호출·잘못된 구문 저장 방지도 함께 소유한다.
- 03: `first,` 입력 직후 `first`로 바뀌어 구분자가 소실되는 사례. 의존성·경로·숫자·정책·환경변수 입력을 소유한다.
- 09: 1100×720 창의 동작 열 계산값 72px. 선언상의 182px가 실제 최종 스타일을 이기지 못한다.
- 10: 실제 한글 IME, 파일 선택/취소, 클립보드, 창 닫기/트레이는 CUA의 네이티브 앱 경로로 관찰한다. 현재 이 OS 관찰은 미실행이며 전역 인수 기준은 완료하지 않았다.
- 일반 `cargo check --workspace` 재검사 통과. 전역 캐시 정리 권한 경고를 유지 기록한다. 일반 UI 재빌드 통과. parity/event는 앞서 기록한 R 결과이며 native로 승격하지 않는다.

## 05번 이후 인증 HTTP 비교 확장

05번에서 같은 facade의 실제 HTTP/invoke 비교를 추가했다. `desktop-e2e` 전용으로 임시 root의 `output/native-http.json`에 OS 할당 loopback port를 기록하며 인증은 유지한다. 기존 invoke 전용이라는 설명은 01번 당시의 기준이다. 일반 빌드의 리스너·CSP는 바뀌지 않는다. `run-native.mjs` 정리는 WebDriver와 HTTP port를 모두 확인한다. [검증 및 정리](05-scheduling.md)를 참조한다.

## 사용자 입력과 분리하는 자동 실행 규칙

2026-10-27 사용자 지시에 따라 전면 창 활성화에 의존하던 실행 방식을 폐기했다. `--activate`, `--observe`, 공유 데스크톱 OS 관찰용 spec은 자동 실행기에서 거부한다. 이전 절의 OS 관찰 명령은 당시 증거 식별용이며 현재 자동 실행 명령이 아니다.

자동 호스트는 표시 전부터 비활성·포커스 불가 상태로 만들고, macOS에서는 투명하고 마우스가 통과하는 렌더링 창만 사용한다. 테스트 프로세스 내부의 AppKit 활성화 요청을 막으며 Dock/트레이도 만들지 않는다. `input-isolation.json`과 매 시나리오의 실제 키 입력 유입 횟수로 이를 검증한다. 사용자 키보드 입력 내용은 수집하지 않는다.

클립보드는 테스트 메모리를 사용하고 실제 파일 선택 권한은 자동 호스트에 주지 않는다. WebDriver 키 입력과 기본 버튼 동작은 테스트 DOM 안에서만 실행한다. 과거 실제 OS 관찰은 기존 빌드 식별과 함께 보존하며, 합성 재현을 새 OS 관찰로 부르지 않는다.

현재 실행 예: `pnpm --dir crates/desktop/ui test:native --no-build --spec=isolation.spec.mjs`. 이후 기능별 `--spec=<name>.spec.mjs`를 같은 실행기로 사용한다. 최초 빌드와 내부 명령은 [10단계](10-native-gates.md)에 기록한다.


## 별도 macOS VM 관찰 경로 — 필수 관찰 완료

공유 데스크톱의 자동 실행은 계속 `desktop-e2e`의 비활성·입력 불가 창만 사용한다. 별도 `desktop-e2e-os` 기능과 `tauri.e2e-os.conf.json`은 실행기와 Rust 진입점에서 `VirtualMac` 여부를 확인한다. [물리 Mac 거부](10-artifacts/os-vm/physical-host-refusal.json)와 [실행기 거부](10-artifacts/os-vm/physical-runner-refusal.json)까지 검증했다.

관찰용 빌드의 명령은 `crates/desktop`에서 `CARGO_TARGET_DIR=../../target/desktop-e2e cargo tauri build --debug --no-sign --bundles app --features desktop-e2e-os --config tauri.e2e-os.conf.json`이다. 복사된 소스는 `MSV_E2E_SOURCE_MANIFEST`로 확인한다. 게스트 `/Users/admin/msv/repo`에서 `node crates/desktop/ui/tests/e2e/run-native.mjs --os-observe --no-build --spec=native-gates.spec.mjs --case=GATE-10`과 같은 실행기의 `--spec=ime-observe.spec.mjs`를 실행해 통과했다. 정확한 빌드·소스 비교와 실패 이력은 [최종 판정](10-native-gates.md)에 있다.

Tart VM은 호스트 화면 창·오디오·USB·클립보드 공유를 끄고 실행했다. macOS 15의 `--net-host` 미지원과 SSH 연결 제한은 기존 이미지의 Guest Agent 제어 경로로 해결했으며 게스트 en0는 관찰 전에 내렸다. 테스트 자료 전용 읽기 폴더와 결과 전용 임시 폴더만 공유했다. 실제 파일 선택, 한글 IME와 변경된 두 입력란의 포커스를 확인했고 사용자 화면·입력·클립보드를 사용하지 않았다. 새로운 자동화 기반을 확장하지 않았으며 [VM은 정지 상태](10-artifacts/os-vm/vm-stop.json)다. [실제 환경](10-artifacts/os-vm/vm-setup.json)과 [최종 판정](10-native-gates.md)을 따른다.
