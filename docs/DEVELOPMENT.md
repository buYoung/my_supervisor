# Development Guide

`my-supervisor` 프로젝트의 로컬 개발 환경 구성, 빌드, 테스트, 디버깅 방법을 정리합니다. 현재 리포지토리는 Cargo workspace와 데스크톱 UI용 독립 pnpm 프로젝트로 구성되어 있으며, 상세 빌드·테스트 플로우는 **PoC 진입 이후** 본 문서를 갱신합니다.

관련 문서: [아키텍처](./ARCHITECTURE.md) · [로드맵](./ROADMAP.md) · [설계 결정](./DESIGN_DECISIONS.md) · [API 레퍼런스](./API.md)

---

## 1. 전제 도구

| 도구 | 역할 | 설치 |
|---|---|---|
| Rust / Cargo | Rust workspace 빌드·테스트 | `rustup` 또는 OS 패키지 매니저 |
| [pnpm](https://pnpm.io/) | 데스크톱 UI 의존성 관리 (`crates/desktop/ui`) | 공식 설치 안내 참조 |

---

## 2. 초기 세팅

리포지토리를 클론한 뒤 한 번만 수행합니다.

```bash
# 데스크톱 UI 의존성 설치
pnpm --dir crates/desktop/ui install
```

---

## 3. 워크스페이스 구조

Rust 크레이트 의존성은 `Cargo.toml`의 workspace members로 관리합니다. 데스크톱 UI는 `crates/desktop/ui` 하위의 독립 pnpm 프로젝트이며, Tauri 설정의 `beforeDevCommand` / `beforeBuildCommand`가 필요한 UI 명령을 실행합니다.

루트 workspace 내부는 Hexagonal 레이어대로 중첩 폴더 구조:

```
├── Cargo.toml            # Rust workspace 루트
├── Cargo.lock
├── crates/
│   ├── core/             # 도메인 + port trait
│   ├── application/      # use case (ports 에만 의존)
│   ├── shared/           # wire types (HTTP/WS DTO, 설정 스키마)
│   ├── config/           # ConfigSource 구현 (TOML + watch)
│   ├── infra/
│   │   ├── sqlite/       # StateRepository + JobRepository
│   │   ├── http/         # HttpServer (axum + WS)
│   │   ├── logging/      # LogSink
│   │   └── scheduler/    # Scheduler
│   ├── platform/
│   │   └── macos/        # launchd, kqueue, macOS 프로세스 제어
│   ├── daemon/           # 공통 데몬 런타임 lib + thin bin `msv-daemon`
│   ├── cli/              # bin `msv` — 데몬 런타임의 기본 접속값 재사용
│   └── desktop/          # Tauri Rust crate
│       ├── Cargo.toml
│       ├── tauri.conf.json
│       ├── src/          # Rust Tauri entry
│       └── ui/           # React/Vite UI
```

`crates/desktop/ui/src/` 는 feature 단위:

```
features/{processes,jobs,logs,daemon,settings}/
components/ui/            # shadcn 공용 (다크·라이트 양립 CSS 변수)
services/                 # HTTP/WS 클라이언트
shared/                   # 훅·타입·유틸 (theme.css — 토큰 단일 출처)
```

각 레이어의 책임과 의존성 방향은 `ARCHITECTURE.md §3`, 설계 근거는 `DESIGN_DECISIONS.md DD-017 ~ DD-024` 참조.

**Workspace members 선언 (`Cargo.toml`):**

```toml
[workspace]
members = [
    "crates/core",
    "crates/application",
    "crates/shared",
    "crates/config",
    "crates/infra/*",
    "crates/platform/*",
    "crates/daemon",
    "crates/cli",
    "crates/desktop",
]
```

Cargo 패키지명은 `my-supervisor-` prefix (예: `my-supervisor-core`, `my-supervisor-platform-linux`, `my-supervisor-app-daemon`).

---

## 4. 빌드 / 테스트 / 실행

기본 단위는 Cargo workspace입니다. UI 명령은 `crates/desktop/ui`에서 직접 실행하거나, `cargo tauri dev/build`가 Tauri 설정을 통해 실행합니다.

### Rust 크레이트

```bash
# 전체 워크스페이스 빌드
cargo build --workspace

# 레이어별 개별 빌드 예시
cargo build -p my-supervisor-core
cargo build -p my-supervisor-application
cargo build -p my-supervisor-platform-linux     # 타겟 OS 에 해당하는 것만
cargo build -p my-supervisor-infra-sqlite
cargo build -p my-supervisor-infra-scheduler    # cron/interval/one-shot/의존성 스케줄러

# 바이너리 (app 레이어)
cargo build -p my-supervisor-app-daemon         # → bin `msv-daemon`
cargo build -p my-supervisor-app-cli            # → bin `msv`

# 유스케이스 단위 테스트 (OS 없이 in-memory adapter 로)
cargo test -p my-supervisor-application

# 플랫폼 adapter 테스트 (해당 OS 에서만 실행됨)
cargo test -p my-supervisor-platform-linux

# 전체 테스트
cargo test --workspace

# Server 배포 산출물 — `target/release`에 네 binary를 같은 target/profile로 생성
cargo msv-release

# 린트 / 포맷
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

### 프론트엔드 (React + Vite)

위치: `crates/desktop/ui/` (`ARCHITECTURE.md` §3 · §4.8 참조).

```bash
# 개발 서버
pnpm --dir crates/desktop/ui dev

# UI 프로덕션 빌드
pnpm --dir crates/desktop/ui build

# 타입 체크
pnpm --dir crates/desktop/ui typecheck
```

### Tauri 앱

```bash
# Tauri 명령은 desktop crate에서 실행한다.
cd crates/desktop

# Tauri 개발 모드 (프론트엔드 dev 서버 + Rust 쉘)
cargo tauri dev

# 배포용 패키징. 먼저 workspace root에서 동일 target의 helper를 준비한다.
cd ../..
cargo msv-release
cd crates/desktop
cargo tauri build
```

`cargo msv-release`가 server 배포의 공식 진입점입니다. 완료 후
`target/release/msv-daemon`, `target/release/msv`,
`target/release/msv-log-proxy`, `target/release/msv-group-reaper` 네 파일이
모두 있어야 합니다. `msv-daemon`은 시작할 때 자신의 절대 경로와 같은
디렉터리에 있는 두 helper를 검증하여 runtime에 주입합니다. 누락되었거나
실행 권한이 없으면 시작이 실패하며, macOS가 서명 문제로 helper 실행을
거부하면 detached launch가 해당 helper 경로를 포함한 오류로 실패합니다.

Tauri release build는 위 release helper를
`crates/desktop/binaries/<helper>-<target-triple>`에 복사하고,
`externalBin`으로 bundle에 포함합니다. 따라서 `cargo tauri build`를 단독으로
실행해 helper를 새 profile로 만들지 않습니다. 다른 target을 빌드할 때는
해당 target으로 `cargo msv-release --target <target-triple>`를 먼저 실행해야
합니다.

---

## 5. 디버깅

- **Rust 로그 레벨**: `RUST_LOG=debug`, `RUST_LOG=my_supervisor_app_daemon=trace` 등 crate 별 필터 사용 (`tracing_subscriber`의 `EnvFilter` 규칙). Rust 의 crate path 는 하이픈이 언더스코어로 변환된다는 점에 유의.
- **데몬 프로세스 상태**: `msv daemon status` (CLI), WebUI `/api/v1/daemon/status`.
- **프론트엔드 개발 서버**: Vite 기본 포트는 데몬(`127.0.0.1:9876`)과 분리. Tauri dev 모드에서는 Tauri 설정에 정의된 dev URL로 WebView가 연결.
- **Windows WebView2**: 설치 여부와 버전은 `Microsoft Edge WebView2 Runtime`으로 확인. PoC 중 최소 버전 기준선 확정.

---

## 6. 코드 스타일

| 영역 | 도구 | 비고 |
|---|---|---|
| Rust | `rustfmt`, `clippy` | 커밋 전 `cargo fmt` + `cargo clippy` 통과 기대 |
| TypeScript / React | `eslint`, `prettier` | 구성은 PoC 시 확정, shadcn/ui 가이드 준용 가능 |
| Markdown (이 문서 포함) | 줄바꿈·코드블록 일관성만 유지 | 린터 미도입 |

세부 규칙(허용 경고, 스타일 예외 등)은 해당 설정 파일(`rustfmt.toml`, `.eslintrc`)이 추가되는 시점에 본 문서도 갱신합니다.

---

## 7. 커밋 메시지 컨벤션

초기 커밋(`feat(프로젝트 초기 설정)`)을 기준으로 [Conventional Commits](https://www.conventionalcommits.org/) 형식을 따르되, 스코프·본문은 한국어/영어 혼용을 허용합니다.

```
<type>(<scope>): <summary>

[본문 — 선택]
[footer — 선택, 예: BREAKING CHANGE, 이슈 번호]
```

- **type**: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `build`, `ci`, `perf` 등 표준 타입
- **scope**: 변경 영역. 레이어 / crate 경로를 간결히 표기 — `core`, `application`, `shared`, `config`, `infra-sqlite`, `infra-http`, `infra-logging`, `infra-scheduler`, `platform-linux`, `platform-macos`, `platform-windows`, `app-daemon`, `app-cli`, `app-desktop`, `ui`. 기능 영역(`jobs`, `theme`, `logging`) 이나 한국어도 허용
- **summary**: 현재형 동사 한 줄 요약

예시:

```
feat(app-daemon): 프로세스 신원 확인 3종 세트 구현
fix(app-cli): ps 명령의 JSON 출력에서 null 필드 누락 수정
docs(architecture): API 오류 포맷 §5.4 신설
refactor(core): LifecycleController port 시그니처에 probe_alive 추가
feat(infra-scheduler): cron 5-field + interval trigger 평가 초기 구현
feat(ui/jobs): Job Runs 탭 가상화 테이블 추가
```

---

## 8. 자주 묻는 이슈

PoC·MVP 진행 중 발견되는 빈발 이슈를 축적합니다. 아래는 틀만 유지합니다.

- **Q. Tauri dev 모드에서 WebView가 로드되지 않습니다.**
  - A. *PoC 중 실제 케이스 수집 후 해결책 기록.*
- **Q. macOS에서 `codesign` 관련 경고가 뜹니다.**
  - A. *Production 단계 패키징 작업 시 기록.*
