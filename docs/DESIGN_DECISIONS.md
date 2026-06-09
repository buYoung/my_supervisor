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

## DD-002: 데몬과 UI — core 공유, 호스트 이원 (개정)

**결정(개정):** 핵심 로직은 `core` + `application` **공유 라이브러리 crate** 에 둔다. 이 코어를 **두 호스트** 가 각각 임베드한다.

- **`app/desktop` (Tauri, 데스크톱)** — 코어를 **인프로세스로 임베드** 하고 tray 로 상주한다. **별도 데몬 프로세스를 spawn 하지 않는다.** WebView UI · macOS 자동화 권한 · tray · 알림 · autostart 담당.
- **`app/daemon` (헤드리스 바이너리, 서버)** — 같은 코어를 임베드한다. Tauri · GUI 의존 없음. SSH/컨테이너에서 운영 기둥을 돌린다.

양쪽 호스트 모두 axum HTTP/WebSocket 서버를 내장하며, CLI · 외부 브라우저가 동일 API 로 접속한다(DD-003 유지).

**맥락(개정 배경):** 초기 결정은 "데몬은 **항상** 별도 바이너리이고 Tauri 가 그 데몬을 spawn·접속" 이었다. 그러나 (1) 제품을 운영 + 자동화 **동등 이원**(DD-026)으로 재정의하면서 자동화(윈도우/핫키)는 **GUI 세션 전용** 이 되었고, (2) 데스크톱에서 별도 데몬을 spawn·중복방지·재연결하는 복잡도가 개인용 도구에는 과했다.

**핵심 분할 (이 개정의 근거):**

| 기둥 | 헤드리스 | 자연스러운 호스트 |
|---|---|---|
| 자동화 (윈도우/핫키) | **불가** (GUI 세션 · TCC 권한 필수) | `app/desktop` (권한 받는 네이티브 프로세스) |
| 운영 (프로세스/Job) | 가능 | 데스크톱은 `app/desktop`, 서버는 `app/daemon` |

자동화는 GUI 세션을 요구하므로 데스크톱 Tauri 호스트가 자연스럽고, 운영만 필요한 서버는 헤드리스 데몬으로 간다. **둘 다 같은 코어** 라 Hexagonal 로 거의 공짜다.

**창 닫힘 / 생명주기:** 데스크톱에서 창을 닫아도 Tauri 가 tray 로 상주하므로 supervision 이 유지된다(`tauri-plugin-single-instance` 단일 인스턴스, `tauri-plugin-autostart` 로그인 재실행). **포기하는 것은 UI 크래시로부터의 프로세스 격리 하나** 뿐이며, 이는 "시스템앱 수준 견고함" 으로 일상 개인용 기준에서 수용한다.

**대안:**
- Tauri 가 별도 데몬을 spawn·접속(원안). **기각** — 데스크톱에 불필요한 spawn/중복방지/재연결 복잡도.
- Tauri 폐기 + 데몬 + 브라우저(SSR/CSR). **기각** — 자동화 권한 · 패키징(.app) · tray 를 데몬이 직접 떠안게 되어 복잡도가 사라지지 않고 이동할 뿐이고, 데스크톱 "앱" 경험도 상실. SSR 은 로컬 단일 사용자에 실익이 없다.

**DD-003 과의 관계:** DD-003(HTTP/WS 통신)은 유지. "별도 프로세스 spawn 여부(DD-002)" 와 "프론트가 HTTP 를 쓰는가(DD-003)" 는 분리된 결정이다.

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

**결정:** 같은 `core`/`application` 을 두 호스트로 패키징 (DD-002).

- Desktop: Tauri 앱 (코어 임베드) + CLI (통합 인스톨러)
- Server: 헤드리스 데몬 (코어 임베드) + CLI (경량 설치 스크립트 / Docker)

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

## DD-021: 라이트/다크 테마 동등 지원

**결정:** 다크 모드와 라이트 모드를 **동등**하게 1급 지원한다. 어느 한쪽이 "1순위" 가 아니며, 기본값은 `auto` (OS `prefers-color-scheme` 추종). 디자인 자산·시안 승인 시 두 모드 스크린샷을 반드시 동시 확인한다.

**맥락:** 초기 기술 감각은 "개발자 대상, 다크 우선" 이었으나 실제 사용 맥락이 다양함 — 밝은 사무실 환경, 프로젝터 데모, 접근성 요구(대비), 그리고 외부 사용자 확대 시 라이트 선호층 비중이 무시 못 할 수준. 또한 UI 가 shadcn/ui 기반(DD-020)이라 두 모드 지원이 이미 기본기.

**이유:**
- **토큰 이원화 비용 < 후속 하드코드 제거 비용**: shadcn/ui 의 CSS 변수 규약을 따르면 두 모드 지원에 들어가는 증분 비용은 팔레트 2 세트 정의 + 시안 2 장 확인뿐. 반면 "다크 우선" 으로 출발해 하드코드 색이 컴포넌트에 누적되면 나중에 추출 비용이 급격히 커진다.
- **접근성**: 라이트 모드는 대비·색맹 테스트 시 놓치기 쉬운 경계선을 드러내주는 2차 검증 수단이기도 하다.
- **Tauri WebView·브라우저 양립과 무관**: 테마는 번들 내 CSS 변수 토글이므로 두 실행 환경 모두 동일.

**운영 규약:**
- 토큰 단일 출처: `packages/ui/src/shared/theme.css` 의 `:root` / `[data-theme="dark"]`
- 허용 색 사용법: 오직 CSS 변수를 통해서만. 하드코드 `#rrggbb` 는 lint 금지
- 시안 승인 절차: Figma 또는 Claude Design 초안이 들어오면 **두 모드 동시 스크린샷 필수**. 한쪽만 검토하고 다른 쪽은 나중에 맞추는 순서는 금지 (하드코드 유입 방지)
- 접근성: 두 모드 모두 WCAG AA 기준 대비 확인

**대안:**
- 다크 전용. **기각** — 확장성·접근성·사용 맥락 다양성 상실.
- 라이트 전용. **기각** — 주 사용자층(개발자) 의 장시간 세션에 눈 피로.

---

## DD-022: Cron/배치 기능은 데몬 내장 (OS 위임 배제)

**결정:** `my-supervisor` 는 cron/interval/one-shot/dependency 기반 배치 스케줄링을 **데몬 자체에 내장**한다. Linux crontab / systemd-timer / macOS launchd calendar / Windows Task Scheduler 에 위임하지 않는다.

**맥락:** 제품 범위 확장 요구 — "PM2 유형 + cron 배치 겸용". 실행 모델을 두 가지로 좁혀 검토: (A) 자체 내장 vs (B) OS 위임.

**이유:**
- **포지셔닝 일관성**: 이 제품의 가치 명제가 "systemd/launchd/Task Scheduler 를 GUI 로 감춘다" 인데, Jobs 에서만 다시 OS 도구로 내려가는 건 스토리 불일치.
- **로그·이벤트 파이프라인 단일화**: Job 실행 로그를 기존 `LogSink` / WebSocket 이벤트 버스 / SQLite 이력과 동일 경로로 흘려보낼 수 있다. OS 위임 시 `journalctl` · Task Scheduler 로그 · launchd logs 를 각각 끌어와 정규화해야 함.
- **cross-platform 동일성**: cron 5-field 문법을 모든 OS 에서 동일하게 파싱. OS 위임 시 Task Scheduler XML 과 crontab 문법 변환 레이어가 필요.
- **UX 통합**: Process 와 Job 을 같은 UI 에서 관리 가능. Trigger Now, Cancel Run, Run Detail 같은 상호작용이 네이티브로 붙는다.

**감수한 트레이드오프:**
- **Daemon 다운 시 스케줄 미실행**: 데몬이 내려가 있으면 예약된 run 이 발화하지 않는다. 이는 Phase 3 의 "missed-runs 백필" 기능으로 완화 예정 (daemon 재기동 시 건너뛴 run 재실행 옵션). MVP 단계에선 감수.
- **타이머 정확도**: tokio 기반 타이머가 OS 커널 타이머보다 정밀도가 낮지만, cron ± 1 s 수준이면 배치 워크로드에 충분.

**DD-010(시스템 서비스 통합) 과의 관계:** DD-010 은 **데몬 자체를 OS 서비스로 등록**하는 얘기 (부팅 시 자동 기동). DD-022 는 데몬 안의 **Job 스케줄링** 얘기. 충돌하지 않으며, DD-010 과 결합했을 때 "OS 가 데몬을 살려두고, 데몬이 Job 을 실행" 으로 자연스러움.

**대안 (B) OS 위임.** **기각** — 위 로그·UX·포지셔닝 이유.

---

## DD-023: Job 과 Process 는 별도 도메인 엔티티

**결정:** `Job` 과 `Process` 를 구분되는 도메인 엔티티로 모델링한다. 공통 상위 `SpawnSpec` 추상화는 Phase 3 까지 보류.

**맥락:** 두 개념은 표면적으로 비슷하다 (command, args, env, cwd, 로그 수집). 그러나 **라이프사이클·UX·이력 의미가 다르다**.

| 속성 | Process | Job |
|---|---|---|
| 기대 수명 | 무기한 (내려가면 장애) | 유한 (실행→종료 정상) |
| 재시작 | 크래시 시 자동 + backoff | 다음 trigger 에서 재실행 |
| 상태 머신 | `starting → running → stopping → stopped` (+ `crashed`) | `pending → running → succeeded \| failed \| cancelled \| skipped` |
| 이력 의미 | "얼마나 오래 살았나" + 재시작 횟수 | "각 실행의 성공/실패, 소요 시간" |
| UX | 목록·상태 뱃지·로그 follow | 스케줄 테이블·Run 이력·Trigger Now |

**이유:**
- **상태 머신 혼합 금지**: 한 엔티티에 두 머신을 섞으면 UI 가 "지금은 어떤 의미의 running 인가?" 를 계속 판단해야 함. Type-safe 분리가 유리.
- **이력 쿼리 분리**: `SELECT * FROM processes WHERE restart_count > 0` 와 `SELECT * FROM job_runs WHERE state = 'failed'` 는 전혀 다른 도메인 질문. 테이블 분리가 자연스러움.
- **UI 모듈 대칭**: `features/processes` vs `features/jobs` 가 백엔드 엔티티 분리와 1:1 대응.

**공통 속성 중복에 대한 대응:**
- Phase 2 MVP: 두 엔티티에 spawn 관련 필드(command/args/env/cwd/timeout) 를 각자 가짐. 중복 ~5 필드, 관리 가능한 수준.
- Phase 3 이후 중복이 늘면 `SpawnSpec` 으로 추출 검토. 섣부른 추상화 금지.

**대안:**
- 단일 `Process` + `trigger: autostart | manual | cron | …` 필드. **기각** — 상태 머신 혼합 문제.
- 상위 `Schedulable` trait 추상화. **기각** — MVP 에는 복잡도만 증가.

---

## DD-024: Job 의존성은 MVP 에 AND + on-success 만

**결정:** Job 의 의존성 트리거 (`DependsOn([job_id, …])`) 는 MVP 에서 다음 형태만 지원:

- 다중 의존: **AND 시맨틱** (모두 성공해야 실행). OR 은 Phase 3.
- 상태 조건: 기본 **on-success**. 실패 시 기본 skip. `on_dependency_failure = "run_anyway"` 옵션으로 실패 무시 실행 가능.
- 등록 시 **순환 감지** → `422 cycle_detected`. 수락된 Job 그래프는 항상 DAG.
- Job 삭제 시 downstream 의존 존재하면 `409 has_dependents`. `--force` 시 의존 해제 후 삭제.
- 한 Job 은 **단일 trigger 타입**만 선택 (cron/interval/one_shot/depends_on 중 하나). 혼합 (예: "cron 스케줄 + 의존 완료 중 먼저 오는 쪽") 은 Phase 3.

**맥락:** 사용자 요구로 의존성 트리거가 추가되었다. DAG 스케줄러 영역은 범위가 빠르게 커지므로 (시각화, OR, on-failure 폭포, 부분 의존, 재시도, …) MVP 는 안전한 최소 형태만 포함.

**이유:**
- **AND + on-success 가 "보통의 배치 파이프라인" 시맨틱**: 백업 → 검증 → 업로드 같은 체인에서 "앞 단계 성공 시 다음" 이 압도적 다수.
- **순환 감지는 필수**: 데이터 무결성 · 데드락 방지. 등록 시 수행하면 런타임 비용 0.
- **혼합 trigger 배제**: 문법·UX·엣지 케이스가 기하급수적. MVP 에서 없어도 대부분 사용자 시나리오 커버.
- **Phase 3 확장 여지 확보**: OR, on-failure, DAG 시각화, 백필 은 이후 점진 추가. 모델 레벨에서 깨지지 않게 오늘 자리만 마련.

**트레이드오프:** on-failure 분기가 필요한 워크플로우(예: "정상이면 A, 실패면 B") 는 MVP 에선 shell 스크립트 래퍼로 우회해야 함. Phase 3 에 정식 지원.

**대안:**
- MVP 부터 OR + on-failure 까지 포함. **기각** — 스코프 폭주.
- 의존성 자체를 Post-Production 으로 미루기. **기각** — 사용자 요구가 명시적.

---

## DD-025: 프로세스 관리 모드 이원화 (Direct / SystemRegistered) — MVP 포함

**결정:** 각 Process 는 프로세스별로 **두 가지 관리 모드** 중 하나를 선택한다. 둘 다 MVP (Phase 2) 에 포함.

- **Direct** *(기본)* — 데몬이 프로세스를 직접 spawn, 감시, 재시작, 로그 수집. `LifecycleController` · `ShutdownSignaler` · `LogSink` 경로.
- **SystemRegistered** — 데몬이 OS 서비스 매니저 (systemd user unit / launchd agent / Windows Service) 에 프로세스를 등록하고, 이후 제어·상태는 OS 경유. 재시작·감시는 OS, 데몬은 조회·UI·로그 집계.

**맥락:** 사용자 층의 "알긴 아는데 매번 귀찮다" 문제를 해결하려면 (1) 데몬이 전부 대신해주는 경로와 (2) OS 의 검증된 supervisor 인프라 (systemd 등) 를 GUI 로 편하게 쓰는 경로 둘 다 필요하다. 둘 중 하나만 지원하면 "왜 내 기존 systemd unit 을 여기서 못 보나" 또는 "GUI 로 간단히 띄우고 싶을 뿐인데 왜 unit 파일을 강제하나" 같은 불만이 생긴다.

**이유:**
- **선택 유연성**: 짧게 띄웠다 내리는 개발 서버는 Direct, 장기 실행 production-grade 프로세스는 SystemRegistered 같이 맥락별 최적.
- **OS supervisor 재활용**: systemd 의 `Restart=on-failure`, `OOMPolicy`, 리소스 제한, 종속성 그래프 같은 성숙한 기능을 재구현하지 않고 위임.
- **데몬 크래시 내성**: SystemRegistered 프로세스는 my-supervisor 데몬이 내려가도 OS 가 계속 관리. "supervisor 를 supervisor 하는" 문제 해결.
- **Hexagonal 와 정합**: `LifecycleController` 와 `ProcessServiceRegistrar` 를 별개 port 로 두어 application 레이어 분기는 `match management_mode` 한 줄.

**핵심 위험 — 재시작 엔진 충돌 (엄수 규칙):**

SystemRegistered 모드에서 my-supervisor 내부의 backoff/crash-loop 로직을 **반드시 비활성** 화한다. 두 재시작 엔진이 동시에 돌면 상태 머신이 깨지고 "재시작이 두 번 일어나는 것처럼 보이는" 유령 버그가 발생한다. 규칙:

1. `ProcessConfig.restart` 는 unit 파일 생성 시 OS native 지시어로 **변환되어 주입** (예: `Restart=on-failure`, `RestartSec=5`). OS 가 재시작 담당.
2. `application::RestartProcess` use case 는 `management_mode = SystemRegistered` 프로세스에 대해 **no-op** 반환. 사용자 UI 에는 "OS 가 자동 재시작을 담당합니다" 안내.
3. `StateRepository` 는 두 경로의 상태를 서로 구분해 저장 (Direct 는 PID + UUID, System 은 `unit_name`).

**다른 핵심 규칙:**

- **유닛 이름 네임스페이스 분리**: 데몬용 unit (`my-supervisor.service`) 과 프로세스용 unit (`my-supervisor-managed-<name>.service`) 이 충돌하지 않도록 prefix 규약.
- **전환 원자성**: Direct ↔ SystemRegistered 전환 중 실패하면 원래 모드로 롤백. 부분 상태 (unit 파일은 생겼는데 registry 업데이트 실패) 를 남기지 않음.
- **유닛 잔재 청소 의무**: `unregister` 실패 시 잔재 unit 파일이 남는다. Phase 3 에 `msv repair system-registrations` CLI 제공. my-supervisor 네임스페이스 prefix 를 기준으로 스캔·정리.
- **로그 경로 이원화**: Direct 는 stdout 파이프 캡처, SystemRegistered 는 journald/launchd logs/Event Log 읽기. `LogSink` port 가 양쪽 다 커버.

**MVP 범위 (Phase 2):**
- 데이터 모델·port·API 자리 확정
- 사용자 주 사용 OS **1 개** 에 SystemRegistered 3 메서드 (register/start/stop) + query_status + tail_logs 완성
- Direct ↔ SystemRegistered 양방향 전환
- 나머지 2 OS adapter 는 Phase 3. MVP 에서는 port 구현만 stub 또는 "not supported" 에러.

**대안:**
- Direct 만 지원. **기각** — OS supervisor 를 GUI 로 다루고 싶은 시나리오 무시, "supervisor 를 supervisor 하는" 문제 미해결.
- SystemRegistered 만 지원. **기각** — OS 서비스 등록은 권한·부수효과·잔재 리스크가 있어 간단한 데브 프로세스에 비용 과다.
- Phase 3 에 SystemRegistered 추가. **기각** — 사용자가 명시적으로 "이 기능까지가 MVP 최소" 로 결정. 단 MVP 에 모든 OS adapter 까지 강제는 범위 폭주라 1 OS 완성 + 모델 자리 확보로 타협.

**DD-010 (시스템 서비스 통합은 Opt-in) 과의 관계:** DD-010 은 (A) 데몬 자체 의 OS 등록 = `AutoStartService`. DD-025 는 (B) 관리 대상 프로세스 의 OS 등록 = `ProcessServiceRegistrar`. 두 축은 독립이며 자유롭게 조합된다 (ARCHITECTURE §11 도입부 참조). DD-010 의 "개별 프로세스를 데몬 외부 시스템 서비스로 등록" 이란 한 줄 언급을 DD-025 가 구체화한다.

---

## DD-026: 자동화 도메인 추가 — `Rule` 단일 엔티티 (동등 이원)

**결정:** 제품을 **운영(Operations)** 과 **자동화(Automation)** 의 **동등한 두 기둥** 으로 재정의한다. 자동화는 `Rule` **단일 도메인 엔티티** 로 모델링하며, `Process` · `Job` 과 병렬인 제3 엔티티다. 자동화는 부가 기능이 아니라 운영과 같은 1 급 영역이다.

**맥락:** 초기 범위는 PM2 형 프로세스 관리 + cron 형 배치였다. 사용자 결정으로 범위를 Hammerspoon 영역(이벤트 기반 OS 자동화, 윈도우 관리, 글로벌 핫키)까지 확장하고, 무게중심을 한쪽에 두지 않는 **동등 이원** 으로 잡았다.

**모델:** `Rule = Trigger(이벤트) + Action[]`.

| 구성 | 종류 (MVP 방향) |
|---|---|
| Trigger | 파일/폴더 변경, 시스템 이벤트(USB · WiFi · 화면 잠금 · 앱 실행), 글로벌 핫키 |
| Action | 프로세스 start/stop, Job 트리거, 명령 실행, 윈도우 액션(이동 · 리사이즈 · 레이아웃) |

**이유:**
- **단일 엔티티의 일관성**: "이벤트 → 액션" 이라는 한 가지 멘탈 모델로 자동화 전체를 표현. 윈도우 · 핫키도 별도 도메인으로 쪼개지 않고 Rule 의 트리거/액션으로 흡수한다.
- **DD-023 의 연장**: `Process`(무기한 수명) ≠ `Job`(유한 run 의 성공/실패) ≠ `Rule`(이벤트 발화 → 액션). 라이프사이클·이력 의미가 셋 다 달라 별도 엔티티가 타입 안전하다.
- **이력 의미 분리**: Rule 은 활성/비활성 상태 + **발화별 실행 이력**(언제 어떤 이벤트로 발화했고 액션이 성공했는가)을 가진다. Process 의 "얼마나 살았나", Job 의 "각 run 의 성패" 와 또 다른 도메인 질문이라 테이블·쿼리가 분리된다.

**동등 이원 IA:** 운영(Processes · Jobs · Logs) + 자동화(Rules) + 공통(Daemon · Settings).

**대안:**
- 윈도우 관리 · 핫키를 각각 독립 도메인으로 분리. **기각** — 도메인이 3~4 개로 분화되어 MVP 복잡도만 증가. 단일 Rule 로 충분히 표현된다.
- Rule 을 Job 에 흡수(트리거 종류만 추가). **기각** — 시간 기반과 이벤트 기반 트리거의 의미·이력이 섞인다 (DD-028).

**관련:** DD-027(자동화 OS 정책), DD-028(트리거 분리), DD-029(권한 모델).

---

## DD-027: 자동화 OS 정책 — 윈도우/핫키는 macOS 전용 (단일 구현 seam, stub 없음)

**결정:** 윈도우 관리 · 글로벌 핫키 · macOS 고유 시스템 이벤트는 **여러 OS 가 구현하도록 강제하는 크로스 플랫폼 port 로 만들지 않는다.** 대신 `core::ports` 에 trait 를 두되 **구현자가 `platform/macos` 하나뿐이고 Linux/Windows stub 을 만들지 않는** *단일 구현 seam* 으로 둔다. 프로세스·배치 코어와 크로스 플랫폼 이벤트 소스(파일/폴더 watch, 타이머)는 기존대로 여러 OS 가 구현하는 port 로 둔다.

**맥락:** 이 저장소의 Hexagonal 원칙(DD-017)은 *"같은 개념을 OS 마다 다른 커널 API 로 구현한다"* 를 전제한다. 이 전제는 프로세스 코어에는 성립하지만(같은 "tied 모드" 를 Linux `PR_SET_PDEATHSIG` / Windows Job Object / macOS shutdown hook 으로 구현), **윈도우/핫키에는 깨진다** — macOS Accessibility(AX API) · X11/Wayland · Windows 는 윈도우·입력 모델의 패러다임 자체가 다르다. 크로스 플랫폼 윈도우 추상화는 누수가 심하기로 악명 높다.

**미러링 함정과 그 회피:** 함정은 *여러 OS 가 구현해야 하는 trait + 빈 stub* 을 만들어 **존재하지 않는 공통 개념을 강제** 하는 것이다(DD-023 "섣부른 추상화 금지" 위반). 이를 피하되 DD-017/018 의 두 불변식을 깨지 않으려면 **DI seam 과 크로스 플랫폼 port 를 구분** 한다:

- **trait 는 `core::ports` 에 둔다** — `WindowAutomation` · `HotkeyBinder` · `SystemEventWatcher`. **단 구현자는 `platform/macos` 단 하나이고 Linux/Windows stub 을 만들지 않는다.** 이것이 "여러 OS 구현 강제 + 빈 stub"(미러링 함정)과의 결정적 차이다.
- **`application` 은 `Option<&dyn WindowAutomation>` 형태로 소비한다** — `#[cfg]` 없음, `platform/*` 의존 없음. (DD-017: `application` 은 `core` 만 의존 / DD-018: cfg 는 `application` 에 등장하지 않음 — **둘 다 보존**)
- **`app/daemon` 의 DI 조립부에서만 `#[cfg(target_os = "macos")]`** 로 macOS 에선 `Some(MacWindowAutomation)`, 그 외 OS 에선 `None` 을 주입한다. cfg 는 여기에만 등장한다(DD-018 의 "cfg 는 `app/daemon` 에만" 규칙 준수).
- **비-macOS 빌드** 에선 주입이 `None` 이므로, 윈도우/핫키 액션은 Rule **등록 시점에** `not supported on this platform` 으로 거부된다(조용한 무시 금지).

**경계 (port 종류 · 구현):**

| 영역 | port 종류 | 구현 |
|---|---|---|
| 프로세스 액션 (start/stop) | 크로스 플랫폼 port (다중 OS 구현) | 기존 `LifecycleController` · `ShutdownSignaler` 재사용 |
| 이벤트 소스 — 파일/폴더 watch, 타이머 | 크로스 플랫폼 port (다중 OS 구현) | `core::ports::EventSource` (adapter: `notify` 기반) |
| 윈도우 제어 | 단일 구현 seam (stub 없음) | `core::ports::WindowAutomation` → `platform/macos` (AX API) |
| 글로벌 핫키 | 단일 구현 seam (stub 없음) | `core::ports::HotkeyBinder` → `platform/macos` (event tap) |
| macOS 고유 시스템 이벤트 | 단일 구현 seam (stub 없음) | `core::ports::SystemEventWatcher` → `platform/macos` (NSWorkspace/IOKit) |

**트레이드오프:** 향후 Windows/Linux 윈도우 관리 수요가 생기면, 그때 해당 trait 에 OS 별 구현을 **실제로 추가** 한다(빈 stub 을 미리 만들지 않는다). 누수 추상화를 선제적으로 만드는 것보다 정직하다.

**관련:** 앞서 검토된 `LifecycleController` 의 `ChildHandle`/trait 시그니처를 Windows Job Object 모델까지 수용하도록 확정하는 작업은 **프로세스 코어에만** 해당하며(슬라이스 ①), 단일 구현 seam 인 자동화 trait 에는 적용하지 않는다.

---

## DD-028: 트리거 모델 — `Job`(시간 기반) 과 `Rule`(이벤트 기반) 분리

**결정:** 시간 기반 트리거(cron / interval / one_shot / depends_on)는 `Job` 이, 이벤트 기반 트리거(파일·시스템 이벤트·핫키)는 `Rule` 이 담당한다. 둘을 단일 통합 트리거 추상으로 합치지 않는다.

**맥락:** 자동화 추가로 트리거 종류가 시간 + 이벤트로 늘었다. (A) 통합 트리거 추상 vs (B) Job/Rule 분리 를 검토했다.

**이유:**
- **도메인 경계 명확**: `Job` 은 "예정된 시각에 유한 작업 실행"(run 성공/실패 이력 중심), `Rule` 은 "이벤트 발생 시 액션 실행"(발화 이력 중심). 의미가 다르다.
- **DD-023 정신 일관**: `Process` ≠ `Job` ≠ `Rule`. 트리거를 통합하면 런타임마다 "이 트리거는 시간인가 이벤트인가" 를 판단해야 하고 상태/이력 의미가 섞인다.
- **이득 < 비용**: 통합 추상의 유연성 이득보다 경계 흐림 비용이 크다.

**트레이드오프:** "매일 2 시 + 폴더 변경 중 먼저 오는 쪽" 같은 혼합 트리거는 직접 표현 불가. 필요 시 **Rule 이 Job 을 트리거(action)** 하는 식으로 조합한다. 정식 혼합 트리거는 향후 검토.

**대안:** 통합 트리거 모델. **기각** — 경계 흐림, 상태/이력 의미 혼합.

---

## DD-029: macOS 자동화 권한 모델 — Accessibility / Input Monitoring, graceful degradation

**결정:** 윈도우 제어는 **Accessibility 권한**, 글로벌 핫키는 **Input Monitoring 권한** 을 요구한다. 권한 미부여 시 해당 기능만 **graceful degradation**(비활성 + 명확한 안내)으로 처리하고, 나머지 기능은 정상 동작시킨다.

**맥락:** macOS 는 윈도우·입력 제어에 사용자 명시적 권한 부여(TCC)를 요구한다. 권한 없이는 AX API · event tap 이 동작하지 않는다.

**이유:**
- **부분 실패 격리**: 권한 미부여가 앱 전체를 막으면 안 된다. 운영(프로세스/배치)과 크로스 플랫폼 트리거는 권한과 무관하게 동작한다.
- **상태 가시화**: 권한 필요 기능은 UI 에서 상태(부여됨 / 필요함)를 표시하고, 원클릭 시스템 설정 안내를 제공한다.
- **"견고하거나 없거나" 원칙(README 핵심 가치)**: 권한이 없으면 윈도우/핫키 Rule 을 "비활성(권한 필요)" 으로 명확히 표기한다. **조용한 실패는 금지** — 사용자가 "왜 핫키가 안 먹지" 를 추측하게 만들지 않는다.

**DD-011(localhost 바인딩) 과의 관계:** DD-011 은 WebUI 접근 통제 축이고, DD-029 는 OS 자원 접근 권한 축이다. 서로 독립이며, 자동화 권한은 OS TCC 에 위임한다.

---

## 변경 로그

- 2026-04-21: 초기 작성 (DD-001 ~ DD-015)
- 2026-04-21: DD-005 갱신 및 DD-016 추가 (프론트엔드 프레임워크를 React + Vite로 확정)
- 2026-04-21: DD-017 ~ DD-020 추가 (Hexagonal 채택, OS별 crate 분리, Infra crate 카테고리 분리, 프론트엔드 feature-based 모듈)
- 2026-04-23: DD-021 ~ DD-024 추가 (테마 동등 지원, Cron 데몬 내장, Job≠Process 모델링, Job 의존성 MVP 범위)
- 2026-04-23: DD-025 추가 (프로세스 관리 모드 이원화 — Direct/SystemRegistered, MVP 포함, 재시작 엔진 충돌 회피)
- 2026-06-09: DD-026 ~ DD-029 추가 (자동화 도메인/`Rule` 동등 이원, 자동화 OS 정책 — 윈도우·핫키 macOS 전용, 트리거 `Job`/`Rule` 분리, macOS 자동화 권한 모델)
- 2026-06-09: DD-002 개정 (데몬은 항상 별도 바이너리 → `core` 공유 + 호스트 이원: Tauri 데스크톱 임베드 / 헤드리스 데몬. 데스크톱의 별도 데몬 spawn 제거, DD-003 유지)
