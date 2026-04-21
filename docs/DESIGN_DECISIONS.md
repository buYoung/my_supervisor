# Design Decisions

주요 설계 결정의 **근거·대안·트레이드오프**를 기록합니다. 새로운 결정이 추가되면 이 문서에 append합니다.

---

## DD-001: Tauri 채택

**결정:** 데스크톱 GUI 셸로 Tauri v2를 사용한다.

**맥락:** GUI 우선 + 크로스 플랫폼 요구사항. 후보는 Electron, Tauri, native(C#/Swift/GTK), Flutter.

**이유:**
- Electron 대비 바이너리 크기 10배 이상 작음 (수 MB vs 수십 MB)
- Rust 백엔드가 데몬과 기술 스택 통일 가능
- 네이티브 WebView 사용으로 메모리 footprint 적음
- 프론트엔드 프레임워크 자유도 (Svelte/React/Vue 모두 가능)

**대안 비교:**

| 후보 | 채택 안 한 이유 |
|---|---|
| Electron | 바이너리 크기, 메모리 사용량, Node.js 런타임 내장 |
| Native | 3개 OS 분리 개발, 유지보수 부담 |
| Flutter | 데스크톱 지원 상대적으로 미성숙, Dart 스택 분리 |

**리스크:** WebView 버전·렌더링 차이 (WebView2 / WKWebView / WebKitGTK). PoC에서 확인.

---

## DD-002: 데몬과 UI의 분리

**결정:** Tauri 앱 안에 데몬 로직을 넣지 않는다. 데몬은 별도 바이너리.

**맥락:** UI 창을 닫았을 때 관리 중인 프로세스가 죽어선 안 됨. 동시에 서버 배포(Tauri 없이)도 지원해야 함.

**이유:**
- UI 프로세스 ≠ supervisor 프로세스. 생명주기 독립 보장.
- Server 배포에서 GUI 의존성 제거 필요.
- 같은 데몬을 Tauri든 브라우저든 동일하게 소비.

**대안:** Tauri 단일 프로세스 구조. **기각** — 요구사항 위반.

---

## DD-003: Tauri invoke IPC 대신 HTTP/WebSocket 사용

**결정:** 프론트엔드가 데몬과 통신할 때 Tauri의 `invoke` IPC 대신 HTTP/WebSocket을 사용한다.

**맥락:** Tauri WebView·외부 브라우저·CLI가 동일한 데몬을 조작해야 함.

**이유:**
- 프론트엔드 코드 **한 벌**로 Tauri 내부·외부 브라우저 둘 다 지원
- CLI는 `reqwest` 사용으로 구현 단순
- Tauri 종속성 감소 — 미래에 Tauri를 바꾸거나 제거해도 UI 코드 재사용

**트레이드오프:**
- IPC 오버헤드 (로컬 HTTP 비용). 로컬 루프백이라 사실상 무시 가능.
- Tauri의 타입 안전 IPC 기능을 포기. `shared` 크레이트의 타입 + OpenAPI 스타일 regen으로 보완.

---

## DD-004: 모노레포 + 워크스페이스 구조

**결정:** 모든 컴포넌트를 단일 Cargo workspace에 배치. 크레이트 분리는 기능 경계 기준.

**구성:**
- `shared` — API 타입, 설정 스키마, 공통 상수
- `daemon` — supervisor 바이너리
- `cli` — CLI 바이너리
- `desktop` — Tauri 앱 (Desktop 배포 전용)

**이유:**
- 버전 동기화 자동 (같은 release)
- `shared`의 타입 변경이 모든 소비자에게 컴파일 타임에 전파
- 단일 `cargo test`로 전체 테스트
- 개인·소규모 프로젝트에 릴리즈 주기 분리는 오버엔지니어링

**대안:** 레포 분리. **기각** — 현 단계에 불필요한 복잡도.

---

## DD-005: Rust 단일 언어

**결정:** 백엔드·CLI·Tauri 쉘 모두 Rust. 프론트엔드만 TypeScript.

**이유:**
- 모노레포 내 크레이트 공유
- 성능·메모리 프로파일 일관성
- 단일 toolchain

**프론트엔드 스택:** React + Vite + Tailwind. 근거는 DD-016 참조.

---

## DD-006: TOML (설정) + SQLite (런타임 상태) 분리

**결정:** 사용자가 편집하는 설정은 TOML 파일, 런타임 상태는 SQLite.

**이유:**
- TOML: 사람이 읽기 쉬움, 버전 관리 친화, GitOps 가능
- SQLite: 재시작 이력·메트릭·상태 전이의 쿼리 효율
- 역할 분리로 동기화 문제 최소화 — TOML은 source of truth, SQLite는 derived state

**대안:**
- 모두 SQLite: 사람 편집성 저하
- 모두 파일: 쿼리·인덱싱 구현 부담

---

## DD-007: 프로세스 생명주기 모드 (tied / detached)

**결정:** 프로세스별로 `tied` 또는 `detached` 모드 선택. 기본값은 `tied`.

**이유:**
- `tied`: 데몬이 죽으면 자식도 정리 → 좀비·고아 위험 최소화. 일반적 개발 시나리오 기본값으로 안전.
- `detached`: 장기 실행 서비스(예: DB, 메시지 브로커)에서 데몬 재시작 시 연속성 유지.

**구현:**
- Linux: `PR_SET_PDEATHSIG` (tied) vs `setsid` (detached)
- Windows: Job Object with/without `KILL_ON_JOB_CLOSE`
- macOS: shutdown hook 방식 (PDEATHSIG 없음)

---

## DD-008: Subreaper 활용 (Linux)

**결정:** 데몬을 `PR_SET_CHILD_SUBREAPER`로 선언.

**이유:**
- 자식이 double-fork로 데몬에게서 도망쳐도 손자가 init(PID 1)이 아닌 데몬으로 reparent
- PM2 등이 놓치는 고아 프로세스 케이스 확실히 회수
- 추가 비용 거의 없음

**Windows:** Job Object가 자연스럽게 동일 역할.
**macOS:** 해당 기능 없음. Reconciliation 루프로 보완.

---

## DD-009: 신원 확인 3종 세트

**결정:** 관리 중인 프로세스 식별에 PID 단독 사용 금지. 다음 3종 조합.

1. PID
2. 프로세스 시작 시각 (`start_time`)
3. UUID 환경 변수 태그 (`MYSUPERVISOR_PROCESS_ID`)

**이유:**
- PID 재사용으로 인한 오탐지 방지
- 데몬 재시작 후에도 이전 자식 재인식 가능
- `ps`로도 소속 식별 가능 (UUID 태그)

**비용:** spawn 시 UUID 주입, 주기적 검증 루프에서 `/proc/<pid>/environ` 읽기. 무시 가능 수준.

---

## DD-010: 시스템 서비스 통합은 Opt-in

**결정:** 데몬 또는 개별 프로세스의 시스템 서비스 등록은 사용자 명시적 활성화가 필요.

**이유:**
- 기본 설치가 시스템에 침입적이면 안 됨
- user-level 자동 시작과 시스템 서비스는 용도·영향이 다름
- 제거 시 시스템 상태 오염 위험

**두 가지 레벨:**
1. 데몬 자체의 시스템 서비스 등록 (선택)
2. 개별 프로세스를 데몬 외부 시스템 서비스로 등록 (선택)

---

## DD-011: 인증 없는 localhost 바인딩

**결정:** WebUI는 `127.0.0.1`에만 바인딩. 인증 없음. `0.0.0.0` 바인딩 코드 레벨에서 금지.

**맥락:** 단일 유저 · 내 PC · production이지만 개인용.

**이유:**
- 복잡도 최소화
- 로컬 다른 사용자 접근 방지는 OS 권한 모델에 위임
- 원격 필요 시 SSH 터널링 사용

**미래:** 원격 기능 추가 시 인증 레이어는 그때 도입 (Post-Production).

---

## DD-012: 로그 백프레셔 정책 — Drop Oldest

**결정:** 프로세스별 로그 bounded channel이 가득 차면 **가장 오래된 라인을 버린다.** 자식 프로세스는 블록하지 않는다.

**이유:**
- 자식이 pipe write에 블록되면 비즈니스 로직이 멈춤 — 받아들일 수 없음
- 최신 로그가 디버깅에 더 가치 있음
- drop 카운트를 메트릭으로 노출해 관찰 가능

**대안:**
- drop newest: 최신 이벤트 손실이 더 치명적
- 블로킹: 자식 영향으로 제외
- 무한 버퍼: 메모리 폭증 위험

---

## DD-013: Crash Loop 시 자동 복구 중단

**결정:** Crash loop 감지 시 자동 재시작 중단, `Crashed` 상태 고정, 알림 발송. 수동 개입 필요.

**이유:**
- 무한 재시작은 CPU·로그 낭비
- 근본 원인 해결 없이 되살려도 의미 없음
- 알림을 통해 사용자 주의 환기

**파라미터:** 슬라이딩 윈도우 (`crash_loop_window_sec`, `crash_loop_threshold`) 기본값 60초 / 5회.

---

## DD-014: 로그 포맷 — JSON 기본, 텍스트 옵션

**결정:** Production 단계부터 기본 로그 포맷은 JSON. 텍스트 포맷은 선택 옵션.

**이유:**
- 로그 수집 파이프라인 연동 쉬움 (Loki, CloudWatch, ELK)
- 구조화 필드로 검색·필터 용이
- WebUI에서도 파싱하여 구조화 렌더링 가능

**MVP에서는 텍스트로 시작.** 파싱·로테이션 복잡도 초기에 배제.

---

## DD-015: 배포 형태 — Desktop과 Server 두 트랙

**결정:** 같은 데몬 바이너리를 두 방식으로 패키징.

- Desktop: Tauri 앱 + 데몬 + CLI (통합 인스톨러)
- Server: 데몬 + CLI (경량 설치 스크립트 / Docker)

**이유:**
- 본서비스는 Desktop이지만 서버 수요 무시 불가
- 단일 코어 공유로 유지보수 부담 최소화
- GUI 의존성을 Server 빌드에서 배제

---

## DD-016: 프론트엔드 프레임워크 — React + Vite

**결정:** Tauri WebView 및 외부 브라우저용 프론트엔드 번들을 **React + Vite + Tailwind** 스택으로 개발한다.

**맥락:** DD-003 (HTTP/WebSocket 기반 통신)에 따라 프론트엔드 번들은 Tauri 내부·외부 브라우저 모두에서 동일하게 동작해야 하며, Tauri IPC에 의존하지 않는다. 따라서 선택의 기준은 Tauri 호환성보다 **개발 생산성·생태계·팀 숙련도**다.

**이유:**
- 팀 숙련도: React 경험치가 가장 높아 학습 비용 최소
- 생태계: UI 라이브러리(shadcn/ui, Radix, Headless UI 등), 데이터 페칭(TanStack Query), 상태관리(Zustand 등) 선택지 풍부
- Vite: 빠른 HMR, ESM 기반 개발 서버, Rollup 기반 최적화 번들. Tauri 공식 템플릿도 Vite 기반이 표준
- Tailwind와 궁합 좋음 (PostCSS + Vite 플러그인 성숙)
- SPA 단순 배포(정적 파일) → Tauri 번들 + 브라우저 접속 모두 자연스럽게 지원

**대안 비교:**

| 후보 | 채택 안 한 이유 |
|---|---|
| SvelteKit | 번들 크기 이점은 있으나 SSR 지향 구조가 SPA 용도에 과하고 팀 숙련도·생태계가 상대적으로 작음 |
| Next.js | App Router·서버 컴포넌트·RSC 등이 Tauri SPA 사용엔 과도. 로컬 대시보드에 불필요 |
| Solid.js | 성능·모델은 우수하나 생태계 규모·UI 라이브러리 선택지가 초기 단계 |
| Vanilla + 직접 구성 | 개인·소규모 프로젝트에 배보다 배꼽 |

**트레이드오프:**
- React의 런타임 비용·번들 크기는 네이티브 앱 기준으로 크지만, Tauri 기반 로컬 앱에서 체감 차이는 미미
- React 생태계 변경 속도가 빨라 메이저 업그레이드(React 19/20 등) 발생 가능 — 호환성 모니터링 필요

**리스크:**
- Tauri WebView(Windows WebView2 / WKWebView / WebKitGTK) 간 최신 JS/CSS 기능 지원 차이. PoC에서 3개 OS 교차 검증 (ROADMAP Phase 1 §1).
- React 의존성 체인 보안 이슈 — 주기적 `npm audit` / dependabot 권장 (Post-Production).

---

## DD-017: Hexagonal 아키텍처 (Ports & Adapters) 채택

**결정:** 모든 도메인·비즈니스 로직을 `core` + `application` crate 에 격리하고, OS·DB·HTTP 등 기술 세부는 `core::ports` 의 trait 로 추상화한다. 실제 구현은 `platform/*` · `infra/*` · `config` adapter crate 가 담당하며, 최상위 bin (`app/daemon` 등) 만 DI 로 조립한다.

**맥락:** 크로스 플랫폼 supervisor 라는 특성상 같은 개념("프로세스 생명주기", "자동 시작", "graceful shutdown")이 Linux/macOS/Windows 에서 완전히 다른 커널 API 로 구현된다. 분기를 도메인 레이어에 허용하면 코드가 3중 `#[cfg]` 로 뒤덮이고, 테스트도 매 OS 에서 돌려야만 의미를 갖는다.

**이유:**
- **플랫폼-프리 도메인**: `application::RestartProcess` 같은 use case 가 port trait 만 호출 → OS 차이를 모름. 같은 코드 경로가 3개 OS 에서 동일하게 돌아가고, `#[cfg(target_os)]` 분기는 `app/daemon` DI 지점에만 남는다.
- **테스트 경량화**: `InMemoryStateRepository`, `FakeLifecycleController` 같은 가짜 adapter 를 주입해 use case 를 OS 없이 단위 테스트 가능. Phase 2 MVP 단계부터 테스트 피라미드 구성 유리.
- **교체 가능성**: `infra/sqlite` → `infra/postgres`, axum → 다른 서버 프레임워크 전환이 adapter crate 만 손대면 된다. `core` / `application` 무변경.
- **Server 배포 최소화**: `platform/*` · `app/desktop` 이 별도 crate 이므로 `cargo build -p my-supervisor-app-daemon` 이 GUI 의존성을 자연스럽게 배제.

**반대 의견과 반박:**
- "MVP 단계의 use case 가 적은데 core/application 분리는 오버엔지니어링 아니냐" — Phase 3 ~ 4 에서 reconciliation, crash loop, 헬스체크, reload_config, autostart toggle, resource limit 등 use case 수가 급증한다. 나중에 분리하면 adapter 가 도메인에 엉킨 후 되돌리기 어렵다. 빈 crate 를 만들어 두는 비용은 매우 낮음.
- "trait object 오버헤드" — supervisor 워크로드는 IO 바운드. vtable dispatch 비용은 `tokio::process::Command::spawn()` 의 syscall 비용 대비 0에 가깝다.

**단방향 의존 규칙:**

```
app/*  ──▶ application ──▶ core
  │             │            ▲
  │             └── shared ──┤
  │                          │
  ├──▶ infra/*    ────────── │
  ├──▶ platform/* ────────── │
  └──▶ config     ────────── ┘
```

`core` 는 다른 워크스페이스 crate 를 의존하지 않는다. `application` 은 `core` 만. `infra/*` · `platform/*` · `config` 는 `core`(+필요 시 `shared`) 만. `app/*` 이 조립.

**관련 결정:** DD-004 (모노레포 + workspace) 는 유지. 이번 DD 는 그 workspace 내부의 crate 세분화 규칙.

---

## DD-018: OS별 구현을 별도 crate 로 분리

**결정:** OS 에 의존하는 모든 adapter 구현은 `crates/platform/linux` · `crates/platform/macos` · `crates/platform/windows` 3 개 crate 로 **물리적으로** 분리한다. 한 crate 내 `#[cfg(target_os)]` module 분할이 아닌, workspace member 단위 분리.

**맥락:** DD-017 의 하위 결정. 분리 단위를 "crate" 로 할지 "한 crate 내부 module" 로 할지 결정이 필요했음.

**이유:**
- **빌드 타겟 독립**: `platform-linux` crate 는 `nix`, `tracing-journald` 를 의존한다. Windows 빌드에서 이 의존성 트리 자체가 다운로드·컴파일되지 않도록 하려면 crate 경계가 필요 (같은 crate 내 `#[cfg]` 로는 Cargo 가 여전히 의존성 그래프를 해석한다).
- **unsafe / syscall 격리**: OS crate 에 갇힌 `unsafe` 블록은 감사 범위가 명확. 도메인·application 에는 `unsafe` 금지 원칙을 유지하기 쉬움.
- **CI 병렬화**: 플랫폼별 CI job 이 자기 platform crate 만 컴파일·테스트. 3개 OS 러너에서 교차 오염이 일어나지 않음.
- **CFG 분기의 최종 위치**: 한 곳만 — `app/daemon` 의 DI 조립부. 이것이 `#[cfg(target_os)]` 가 등장하는 유일한 곳이라는 규율을 문서와 코드로 강제 가능.

**대안:**
- 단일 `platform` crate + 내부 `#[cfg]` module 분할. **기각** — 위 빌드·의존성 이유.
- 하이브리드 (공통 util 은 `platform-common`, OS별은 별도 crate). **보류** — 현 단계 공통 util 이 없음. 필요하면 추후 추가.

**이름 규약:**
- Cargo 패키지명: `my-supervisor-platform-linux`, `my-supervisor-platform-macos`, `my-supervisor-platform-windows`
- 폴더: `crates/platform/linux/`, `crates/platform/macos/`, `crates/platform/windows/`
- 각 crate 의 `Cargo.toml` 에 `[target.'cfg(target_os = "linux")'.dependencies]` 등을 활용해 타 OS 에서 빌드 스킵.

---

## DD-019: Infra crate 카테고리 분리

**결정:** 기술 스택에 종속적인 adapter 를 책임 단위로 분할한다: `infra/sqlite`, `infra/http`, `infra/logging`. 앞으로 필요 시 `infra/postgres`, `infra/grpc` 등을 같은 패턴으로 추가.

**맥락:** DD-017 의 하위 결정. "infra" 를 한 crate 로 묶는 관행도 있으나, 책임이 다른 기술(DB · 네트워크 · 로깅 파이프라인)을 같은 crate 에 두면 의존성 트리가 불필요하게 커지고 교체도 어렵다.

**이유:**
- **교체 용이성**: SQLite → Postgres 이전이 `infra/sqlite` → `infra/postgres` crate 교체 + `app/daemon` DI 한 줄 수정으로 끝. `application` 레이어 무변경.
- **Feature 단위 옵셔널 빌드**: 향후 Server 배포에서 로깅만 교체하고 싶을 때 (예: systemd-journald-only) `infra/logging` 만 fork 또는 feature flag 로 제어.
- **테스트 주입 단위**: `InMemoryStateRepository` 같은 테스트용 구현을 `infra/sqlite` 와 별개로 `application` 테스트 하네스에 둘 수 있음.

**대안:**
- 단일 `infra` crate. **기각** — 커지면 관리·교체 비용 증가.

---

## DD-020: 프론트엔드 feature-based 모듈 구조

**결정:** `packages/ui/src/` 를 `features/*` + `components/ui` + `services/` + `shared/` 로 구성한다. 각 feature (processes, logs, daemon, settings) 는 자기 UI · 자기 service 호출 · 자기 훅을 자체 디렉터리에 둔다.

**맥락:** DD-016 에서 React + Vite 스택 확정. 이번 DD 는 그 스택 위의 **디렉터리 규약**.

**이유:**
- **백엔드 port 분리와 대칭**: 백엔드가 Hexagonal port 단위로 나뉘어 있으므로, UI 도 feature 단위로 나누면 한 feature 의 변경이 한 폴더 + 한 service 로 수렴한다.
- **shadcn/ui 관행과 호환**: `components/ui` 를 공용 디자인 시스템 위치로 사용하는 shadcn 규약과 자연스럽게 결합.
- **Tauri / 브라우저 양립**: 모든 API 호출이 `services/` 에 집중 → Tauri 번들·외부 브라우저 공용 SPA 를 유지하면서도 환경별 분기가 필요할 때 한 파일만 손대면 됨.
- **확장 경로**: feature 가 커지면 그 안에서 `components/`, `hooks/`, `types.ts` 등으로 자체 서브 구조를 확장. 교차 feature 공유 필요 시 `shared/` 로 승격.

**대안:**
- page-based (`pages/processes.tsx` 등). **기각** — 로직이 페이지 파일에 누적되며 다른 UI (트레이 패널, 데스크톱 모달 등) 로의 재사용 어려움.
- Atomic Design (atoms/molecules/organisms). **기각** — 내부 재사용 규모가 작아 관리 오버헤드가 이득을 넘음.

---

## 변경 로그

- 2026-04-21: 초기 작성 (DD-001 ~ DD-015)
- 2026-04-21: DD-005 갱신 및 DD-016 추가 (프론트엔드 프레임워크를 React + Vite로 확정)
- 2026-04-21: DD-017 ~ DD-020 추가 (Hexagonal 채택, OS별 crate 분리, Infra crate 카테고리 분리, 프론트엔드 feature-based 모듈)
