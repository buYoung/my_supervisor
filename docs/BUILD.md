# CLI·앱 로컬 빌드 가이드

이 문서는 macOS에서 `msv` CLI와 `my-supervisor.app`을 직접 빌드하고 실행하는 절차를 설명합니다. 현재 런타임과 앱 패키징은 macOS만 지원하며, 첫 릴리스 앱은 `arm64`(`aarch64-apple-darwin`) 전용입니다.

## 1. 준비

필수 도구는 다음과 같습니다.

| 도구 | 기준 | 확인 명령 |
|---|---|---|
| Xcode Command Line Tools | macOS SDK와 링커 제공 | `xcode-select -p` |
| Rust stable | Cargo workspace 빌드 | `rustc --version` |
| Node.js | 22 | `node --version` |
| pnpm | 10.11.0 | `pnpm --version` |
| Tauri CLI | 2.11.4 | `cargo tauri --version` |

처음 한 번 다음 명령을 실행합니다.

```bash
xcode-select --install
rustup target add aarch64-apple-darwin
corepack enable
corepack prepare pnpm@10.11.0 --activate
pnpm --dir crates/desktop/ui install --frozen-lockfile
cargo install tauri-cli --version 2.11.4 --locked
```

모든 명령은 별도 표기가 없으면 저장소 루트에서 실행합니다.

## 2. CLI 빌드와 실행

로컬 기능 확인에는 디버그 빌드를 사용합니다. 이 명령은 CLI뿐 아니라 CLI가 접속할 데몬과 macOS 보조 실행 파일도 같은 프로필로 빌드합니다.

```bash
scripts/build-local.sh cli
```

산출물은 다음과 같습니다.

- `target/debug/msv`
- `target/debug/msv-daemon`
- `target/debug/msv-log-proxy`
- `target/debug/msv-group-reaper`

터미널 두 개를 열어 데몬과 CLI를 확인합니다.

```bash
# 터미널 1
./target/debug/msv-daemon
```

```bash
# 터미널 2
./target/debug/msv daemon status
./target/debug/msv --help
```

기본 API 주소는 `http://127.0.0.1:9876`입니다. 데몬은 사용자 데이터 디렉터리를 사용하므로 기존 로컬 데이터에 영향을 주고 싶지 않다면 앱·CLI 동작 검증 전에 별도 테스트 환경을 준비해야 합니다. `MSV_DAEMON_TEST_*` 변수는 디버그 통합 테스트용이며 일반 운영 설정이 아닙니다.

최적화된 CLI와 데몬을 빌드하려면 다음 명령을 사용합니다.

```bash
scripts/build-local.sh cli release
```

산출물은 `target/release/`에 생성됩니다.

## 3. 앱 개발 실행

WebView와 Rust 호스트를 개발 모드로 실행하려면 먼저 실제 디버그 보조 실행 파일을 준비한 뒤 Tauri 개발 서버를 시작합니다.

```bash
scripts/build-local.sh cli
cd crates/desktop
cargo tauri dev
```

Tauri가 `crates/desktop/ui`에서 `pnpm dev`를 실행하고 Vite 개발 서버에 연결합니다. 이 모드는 `.app` 설치 후보를 만드는 대신 빠른 UI·호스트 반복 검증에 적합합니다.

## 4. 로컬 앱 빌드

### 디버그 앱

다음 한 명령이 실제 디버그 보조 실행 파일, UI, Tauri 호스트를 빌드하고 서명되지 않은 `.app`을 만듭니다.

```bash
scripts/build-local.sh app
```

산출물:

```text
target/debug/bundle/macos/my-supervisor.app
```

Finder에서 열거나 다음 명령으로 실행할 수 있습니다.

```bash
open target/debug/bundle/macos/my-supervisor.app
```

### 로컬 릴리스 앱

릴리스 프로필과 실제 사이드카로 서명되지 않은 로컬 검증용 앱을 만듭니다.

```bash
scripts/build-local.sh app release
```

Apple Silicon Mac의 산출물:

```text
target/release/bundle/macos/my-supervisor.app
```

Intel Mac에서는 `aarch64-apple-darwin`으로 교차 빌드하므로 다음 경로를 사용합니다.

```text
target/aarch64-apple-darwin/release/bundle/macos/my-supervisor.app
```

릴리스 앱 빌드는 다음 단계를 자동으로 수행합니다.

1. 빌드 입력의 소스 출처 매니페스트 생성
2. `msv-daemon`, `msv`, `msv-log-proxy`, `msv-group-reaper` 릴리스 빌드
3. Tauri `externalBin` 형식으로 사이드카 준비
4. UI와 `my-supervisor.app` 빌드
5. 빌드 후 소스 출처 매니페스트 재검증

이 결과는 로컬 실행 검증용이며 배포 후보가 아닙니다. 코드 서명, 공증, 스테이플링, DMG 생성·검증은 포함하지 않습니다. 배포 후보 검증은 [프로덕션 준비 문서](./PRODUCTION_READINESS.md)의 별도 게이트를 따릅니다.

## 5. UI만 빌드

Rust 호스트 없이 프론트엔드만 확인할 수 있습니다.

```bash
pnpm --dir crates/desktop/ui typecheck
pnpm --dir crates/desktop/ui build
```

정적 산출물은 `crates/desktop/ui/dist/`에 생성됩니다.

## 6. 문제 해결

### `no such command: tauri`

```bash
cargo install tauri-cli --version 2.11.4 --locked
```

### UI 의존성 또는 잠금 파일 오류

```bash
pnpm --dir crates/desktop/ui install --frozen-lockfile
```

### 보조 실행 파일 누락 오류

`cargo tauri build`를 단독 실행하면 릴리스 사이드카 계약을 충족하지 못할 수 있습니다. 저장소 루트에서 다음 명령을 다시 실행합니다.

```bash
scripts/build-local.sh app release
```

### 소스 출처 매니페스트 불일치

릴리스 빌드 도중 추적 대상 또는 추적되지 않은 빌드 입력이 변경된 경우 실패합니다. 진행 중인 편집이나 생성 작업을 마친 뒤 명령을 처음부터 다시 실행합니다.

### 포트 `9876` 사용 중

이미 실행 중인 `msv-daemon` 또는 데스크톱 앱을 종료한 뒤 다시 실행합니다. 제품 런타임은 루프백 기본 주소를 사용하며 일반 실행에서 임의 포트 변경을 지원하지 않습니다.
