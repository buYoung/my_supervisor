# Development Guide

`my-supervisor` 프로젝트의 로컬 개발 환경 구성, 빌드, 테스트, 디버깅 방법을 정리합니다. 현재 리포지토리는 설계 단계로 워크스페이스 골격과 버전 고정 도구만 구성되어 있으며, 상세 빌드·테스트 플로우는 **PoC 진입 이후** 본 문서를 갱신합니다.

관련 문서: [아키텍처](./ARCHITECTURE.md) · [로드맵](./ROADMAP.md) · [설계 결정](./DESIGN_DECISIONS.md) · [API 레퍼런스](./API.md)

---

## 1. 전제 도구

| 도구 | 역할 | 설치 |
|---|---|---|
| [proto](https://moonrepo.dev/proto) | 언어·런타임 버전 관리. `.prototools`로 Node / Rust / pnpm 버전 고정 | 공식 설치 스크립트 참조 |
| [moon](https://moonrepo.dev/moon) | 모노레포 태스크 러너. `.moon/workspace.yml`로 프로젝트 경계 정의 | `proto install moon` 또는 공식 설치 |

현재 `.prototools`가 고정하는 버전:

```
node = "24.14.0"
rust = "1.94.1"
pnpm = "10.11.0"
```

버전은 진행 단계에 따라 변경될 수 있으며, 변경 시 해당 커밋과 함께 이 문서를 갱신합니다.

---

## 2. 초기 세팅

리포지토리를 클론한 뒤 한 번만 수행합니다.

```bash
# 1) proto 설치 + .prototools에 고정된 Node/Rust/pnpm 설치
./scripts/setup-proto.sh

# 2) moon 기반 워크스페이스 디렉터리 및 설정 파일 생성
./scripts/setup-moon.sh
```

각 스크립트의 동작:

- `scripts/setup-proto.sh` — `proto` 명령 존재 여부 확인 → `.prototools`가 없으면 기본값으로 생성 → `proto install --yes`로 지정 버전 설치. 셸 프로필은 수정하지 않으므로 필요 시 `proto setup`을 별도 실행.
- `scripts/setup-moon.sh` — `moon` 명령 존재 여부 확인 → `apps/`, `packages/`, `crates/` 디렉터리 보장 → `.moon/workspace.yml`, `.moon/toolchains.yml`을 없으면 기본값으로 생성.

> 두 스크립트 모두 **이미 존재하는 설정 파일을 덮어쓰지 않고 재사용**합니다.

---

## 3. 워크스페이스 구조

`.moon/workspace.yml` 기준의 상위 프로젝트 경계:

```
apps/*        # 최상위 애플리케이션 (moon 규약 유지, 현 단계 미사용)
packages/*    # 노드 기반 공유 패키지 — packages/ui (React 프론트엔드)
crates/*      # Rust 크레이트 — Hexagonal 5 레이어
```

`crates/` 내부는 Hexagonal 레이어대로 중첩 폴더 구조:

```
crates/
├── core/                 # 도메인 + port trait
├── application/          # use case (ports 에만 의존)
├── shared/               # wire types (HTTP/WS DTO, 설정 스키마)
├── config/               # ConfigSource 구현 (TOML + watch)
├── infra/
│   ├── sqlite/           # StateRepository + JobRepository
│   ├── http/             # HttpServer (axum + WS)
│   ├── logging/          # LogSink (로테이션·백프레셔; run 단위 아카이브)
│   └── scheduler/        # Scheduler (cron/interval/one-shot/의존성)
├── platform/
│   ├── linux/            # prctl, systemd, journald
│   ├── macos/            # kqueue, launchd
│   └── windows/          # Job Object, Service, Event Log, Task Scheduler
└── app/
    ├── daemon/           # bin `msv-daemon` — DI 조립
    ├── cli/              # bin `msv`
    └── desktop/          # bin — Tauri shell
```

`packages/ui/src/` 는 feature 단위:

```
features/{processes,jobs,logs,daemon,settings}/
components/ui/            # shadcn 공용 (다크·라이트 양립 CSS 변수)
services/                 # HTTP/WS 클라이언트
shared/                   # 훅·타입·유틸 (theme.css — 토큰 단일 출처)
```

실제 디렉터리는 현재 **비어 있고** (`.gitkeep`만 존재), PoC 시작 시 위 구조로 crate 가 추가됩니다. 각 레이어의 책임과 의존성 방향은 `ARCHITECTURE.md §3`, 설계 근거는 `DESIGN_DECISIONS.md DD-017 ~ DD-024` 참조.

**Workspace members 선언 (루트 `Cargo.toml`):**

```toml
[workspace]
members = [
    "crates/core",
    "crates/application",
    "crates/shared",
    "crates/config",
    "crates/infra/*",
    "crates/platform/*",
    "crates/app/*",
]
```

Cargo 패키지명은 `my-supervisor-` prefix (예: `my-supervisor-core`, `my-supervisor-platform-linux`, `my-supervisor-app-daemon`).

`.moon/toolchains.yml`에는 `node`, `rust` 툴체인이 활성화되어 있으며, 세부 옵션은 PoC 중 확정합니다.

---

## 4. 빌드 / 테스트 / 실행

현재 코드가 없어 명령만 패턴으로 정리합니다. PoC 진입 시 각 항목을 실제 명령으로 고정합니다.

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

# Server 빌드 — GUI·desktop crate 배제 패턴
cargo build -p my-supervisor-app-daemon -p my-supervisor-app-cli --release

# 린트 / 포맷
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

### 프론트엔드 (React + Vite)

위치: `packages/ui/` (`ARCHITECTURE.md` §3 · §4.8 참조).

```bash
# 개발 서버
pnpm -C packages/ui dev

# 프로덕션 빌드
pnpm -C packages/ui build

# 타입 체크 / 린트
pnpm -C packages/ui typecheck
pnpm -C packages/ui lint
```

### Tauri 앱

```bash
# Tauri 개발 모드 (프론트엔드 dev 서버 + Rust 쉘)
cargo tauri dev

# 배포용 패키징
cargo tauri build
```

### Moon 태스크

프로젝트가 추가되면 공통 태스크(예: `:build`, `:test`, `:lint`)를 `.moon/tasks.yml`로 집약합니다. 그 시점에 다음 예시가 동작하도록 정비합니다.

```bash
moon run :build
moon run :test
```

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

- **Q. `proto install`이 Rust 설치 단계에서 실패합니다.**
  - A. *PoC 중 실제 케이스 수집 후 해결책 기록.*
- **Q. Tauri dev 모드에서 WebView가 로드되지 않습니다.**
  - A. *PoC 중 실제 케이스 수집 후 해결책 기록.*
- **Q. macOS에서 `codesign` 관련 경고가 뜹니다.**
  - A. *Production 단계 패키징 작업 시 기록.*
