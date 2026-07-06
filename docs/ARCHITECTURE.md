# Architecture

본 문서는 `my-supervisor` 프로젝트의 전체 아키텍처, 컴포넌트 설계, 플랫폼별 구현 전략을 정리합니다.

## 1. 설계 원칙

1. **GUI 호스트와 헤드리스 launcher** — `core`/`application` 과 adapter 조립을 `daemon` 런타임 라이브러리로 모으고, **GUI 가 있는 my-supervisor**(`desktop`, Tauri)는 이를 인프로세스로 재사용한다. **GUI 가 없는 my-supervisor**는 같은 런타임을 `msv-daemon` launcher로 실행한다. Tauri 는 별도 데몬 프로세스를 spawn 하지 않는다(DD-002). 데스크톱에서 창을 닫아도 tray 로 상주해 관리 중인 프로세스는 유지된다.
2. **단일 통신 프로토콜** — Tauri WebView·브라우저·CLI가 모두 동일한 HTTP/WebSocket API를 사용한다(각 호스트가 내장 서버로 제공).
3. **단일 런타임, 두 실행 형태** — Desktop·Server 배포는 동일한 `daemon` 런타임 조립을 공유하고, 실행 형태(`desktop` Tauri 셸 · `msv-daemon` launcher)만 다르다.
4. **GUI 우선, CLI 동등** — 주 사용자는 데스크톱 GUI, 그러나 CLI로도 동일 기능을 수행할 수 있어야 한다.
5. **Opt-in 시스템 통합** — 시스템 서비스 등록은 기본이 아니며 사용자가 명시적으로 활성화한다.
6. **Hexagonal 아키텍처 (Ports & Adapters)** — 도메인 로직은 OS·DB·네트워크 세부를 모른다. 모든 외부 의존은 `core` crate 의 port trait 로 추상화되고, `platform-*` · `infra-*` crate 가 adapter 로 구현한다. `#[cfg(target_os)]` 분기는 `daemon` 런타임 조립부에만 존재한다. 근거: DD-017 ~ DD-019.
7. **Process + Job 이원 모델** — 장시간 실행 supervisor 대상인 **Process** 와 예약/주기/의존성으로 트리거되어 실행 후 종료를 기대하는 **Job** 은 별도 도메인 엔티티로 모델링한다. Job 스케줄링은 OS cron/Task Scheduler 에 위임하지 않고 데몬이 자체 수행한다. 근거: DD-022 ~ DD-024.
8. **프로세스 관리 모드 이원화** — 각 Process 는 **Direct** (데몬이 직접 spawn·감시·재시작) 또는 **SystemRegistered** (OS 서비스 매니저 — systemd user unit / launchd agent / Windows Service — 에 등록 후 OS 가 감시, 데몬은 제어·조회·로그 집계) 중 하나를 프로세스별로 선택한다. 두 모드는 port 레벨에서 분리되며 재시작 엔진 충돌은 설계 시점에 차단한다. 근거: DD-025.
9. **테마 동등 지원** — 다크 모드와 라이트 모드를 동등하게 지원한다. 토큰은 shadcn/ui CSS 변수를 단일 출처로 쓰고, 디자인 시안 승인 시 두 모드 스크린샷을 동시 확인한다. 근거: DD-021.

### 지원 OS 최소 버전

PoC부터 다음 환경을 기준선으로 가정한다. 더 오래된 OS 지원은 수요가 확인되면 Post-Production에서 검토.

| OS | 최소 버전 | 비고 |
|---|---|---|
| Windows | Windows 10 1809 (2018-10) 이상 | WebView2 런타임 설치 필요, Windows 11 권장 |
| macOS | macOS 12 Monterey 이상 | WKWebView / launchd 기반 |
| Linux | Ubuntu 20.04 / Debian 11 / 동등 커널 5.4+ | WebKitGTK 2.36+, systemd 가정 |

PoC의 교차 검증 환경은 이 기준선에서 선정한다.

## 2. 배포 전략

### Desktop 배포 (본서비스)

단일 설치 프로그램이 두 개의 아티팩트를 설치한다 — **데스크톱은 Tauri 앱이 코어를 임베드하므로 별도 `msv-daemon` 을 설치하지 않는다** (DD-002):

- Tauri 앱 — **GUI 가 있는 my-supervisor**. `core`/`application` 임베드 + 내장 HTTP/WS 서버 (crate: `my-supervisor-app-desktop`)
- `msv` — CLI 클라이언트 (crate: `my-supervisor-app-cli`, PATH에 등록)

> 헤드리스 `msv-daemon` (crate: `my-supervisor-app-daemon`) 은 **Server 배포** 전용이다 (아래).

패키지 형식:

| OS | 형식 | 비고 |
|---|---|---|
| Windows | MSI | PATH 등록, 시작 메뉴 |
| macOS | DMG 또는 PKG | `/Applications/my-supervisor.app` + `/usr/local/bin/msv` 심링크 |
| Linux | deb / rpm / AppImage | `.desktop` 파일 포함 |

### Server 배포 (확장)

GUI 의존성 없이 데몬과 CLI만 배포:

- Linux: `curl \| sh` 설치 스크립트, apt/yum 레포 *(추후)*, Docker 이미지
- Windows Server: ZIP 또는 Chocolatey
- 정적 링크 단일 바이너리 지향

## 3. 모노레포 구조

Hexagonal 레이어를 **Cargo workspace 의 개별 crate** 로 분리한다. Turborepo는 `apps/my_supervisor/package.json`을 통해 Cargo workspace 전체를 상위 태스크 그래프에 포함한다. 폴더는 카테고리 하위 중첩(`apps/my_supervisor/platform/linux/`, `apps/my_supervisor/infra/sqlite/`, `apps/my_supervisor/daemon/`).

```
my-supervisor/
├── README.md
├── docs/
├── package.json                     # pnpm + Turborepo 루트 스크립트
├── pnpm-workspace.yaml              # Node 패키지 경계 (apps/*, packages/*)
├── turbo.json                       # Node 패키지 태스크 파이프라인
├── apps/                            # 최상위 애플리케이션 (현 단계 미사용)
├── packages/
│   └── ui/                          # React 프론트엔드 (Tauri WebView·외부 브라우저 공용)
│       ├── package.json
│       └── src/
│           ├── features/
│           │   ├── processes/       # 프로세스 목록·상세·제어
│           │   ├── jobs/            # Jobs(cron/interval/one-shot/의존성) 목록·상세·실행 이력
│           │   ├── logs/            # 로그 뷰어·follow
│           │   ├── daemon/          # 데몬 상태·리로드
│           │   └── settings/        # 설정 편집
│           ├── components/ui/       # shadcn 공용 컴포넌트 (CSS 변수 기반, 다크+라이트 양립)
│           ├── services/            # HTTP/WS 클라이언트
│           └── shared/              # 타입·훅·유틸
├── apps/my_supervisor/
│   ├── package.json                 # Turborepo에서 Cargo workspace를 대표하는 작업 패키지
│   ├── core/                        # 도메인 + port trait (std/serde/uuid 수준만 의존)
│   │   └── src/
│   │       ├── domain/              # Process·ProcessState·RestartPolicy·ChildHandle,
│   │       │                        #  Job·JobTrigger·JobRun·JobRunState·OverlapPolicy,
│   │       │                        #  Rule·RuleTrigger·RuleAction·RuleFire, …
│   │       └── ports/               # LifecycleController, ShutdownSignaler, AutoStartService,
│   │                                #  LogSink, StateRepository, HealthChecker, ConfigSource,
│   │                                #  Scheduler, JobRepository, JobRunner,
│   │                                #  EventSource(크로스플랫폼), WindowAutomation·HotkeyBinder·
│   │                                #  SystemEventWatcher(macOS 단일 구현 seam — DD-027), …
│   ├── application/                 # use case (ports 에만 의존)
│   │   └── src/
│   │       ├── start_process.rs
│   │       ├── stop_process.rs
│   │       ├── restart_process.rs   # backoff + crash loop; OS 동작은 port 호출
│   │       ├── reconcile.rs
│   │       ├── reload_config.rs
│   │       ├── register_job.rs      # 등록 + 순환 감지 (cycle_detected)
│   │       ├── trigger_job.rs       # 수동 트리거·on_overlap 분기
│   │       ├── schedule_tick.rs     # Scheduler 이벤트 수신 → JobRunner 호출
│   │       ├── observe_job_run.rs   # 완료 → 의존 Job 트리거 전파
│   │       └── cancel_job_run.rs
│   ├── shared/                      # wire types (HTTP/WS DTO, 설정 스키마 — serde)
│   │   └── src/
│   │       ├── api.rs               # REST 요청/응답 (Process·Job DTO 포함)
│   │       ├── events.rs            # WS 이벤트 페이로드 (process.* + job.*)
│   │       └── config.rs            # TOML 스키마 ([[process]] + [[job]])
│   ├── config/                      # TOML 파싱·검증·watch (shared::config 재사용)
│   │   └── src/
│   │       ├── loader.rs
│   │       ├── validator.rs
│   │       └── watcher.rs           # notify 크레이트 — ConfigSource 구현
│   ├── infra/
│   │   ├── sqlite/                  # StateRepository + JobRepository 구현 (sqlx/SQLite)
│   │   ├── http/                    # axum HTTP + WS hub (core::ports 소비)
│   │   ├── logging/                 # LogSink 구현 + 로테이션·백프레셔 (file-rotate, tracing)
│   │   └── scheduler/               # Scheduler 구현 (cron 5-field + interval + one-shot
│   │                                #  + dependency 타이머; tokio-cron-scheduler 기반)
│   ├── platform/
│   │   ├── linux/                   # prctl·PDEATHSIG·subreaper, systemd, journald
│   │   ├── macos/                   # kqueue shutdown hook, launchd plist, unified logs
│   │   └── windows/                 # Job Object, Service, Event Log, Task Scheduler, CTRL_BREAK
│   ├── daemon/                      # 공통 데몬 런타임 lib + thin bin (`msv-daemon`)
│   ├── cli/                         # bin (`msv`) — reqwest 클라이언트 (shared 타입 재사용)
│   └── desktop/                     # bin — Tauri shell (tray, invoke, webview)
└── scripts/
    ├── install-server.sh
    └── build-packages.sh
```

**Workspace members 선언**:

```toml
# apps/my_supervisor/Cargo.toml
[workspace]
members = [
    "core",
    "application",
    "shared",
    "config",
    "infra/*",
    "platform/*",
    "daemon",
    "cli",
    "desktop",
]
```

Cargo 패키지명은 `my-supervisor-` prefix (예: `my-supervisor-core`, `my-supervisor-platform-linux`, `my-supervisor-app-daemon`). Rust `use` 경로는 자동으로 `my_supervisor_*` 로 변환된다. 바이너리 이름은 별개로 `msv` (CLI) / `msv-daemon` (데몬) 으로 설정한다.

### 3.1 의존성 방향 (단방향)

```
  cli ─┐
           ├──▶ daemon(runtime) ──▶ application ──▶ core ◀── port trait 정의
  desktop ┘              │              │                    ▲
                             │              └── shared ──────────┤
                             ├──▶ infra/*    ─────────────────── │
                             ├──▶ platform/* ─────────────────── │
                             └──▶ config     ─────────────────── ┘
```

규칙:

- `core` 는 다른 어떤 워크스페이스 crate 도 의존하지 않는다. 외부 crate 도 `serde`·`uuid`·`thiserror`·`tokio`(trait async) 등 의존성이 얇은 것만.
- `application` 은 `core` 만 의존. use case 는 port trait 를 호출할 뿐 adapter 타입을 직접 참조하지 않는다.
- `infra/*` · `platform/*` · `config` 는 `core`(+ 필요 시 `shared`) 만 의존. 서로 의존하지 않는다.
- `daemon` 의 라이브러리 타깃이 공통 조립 지점이다. `core` · `application` · 필요한 `infra/*` · `platform/*` · `config` · `shared` 를 가져와 데몬 런타임을 만든다.
- `desktop` 은 Tauri 셸과 invoke/devBridge만 담당하고, 데몬 런타임 조립은 `daemon` 라이브러리를 재사용한다. `cli` 는 같은 데몬 기본 접속값과 `shared` DTO를 재사용한다.

### 3.2 왜 이 구조인가

- **core / application 의 플랫폼-프리**: OS·DB·HTTP 세부가 도메인으로 누출되지 않아 Linux/macOS/Windows 가 각기 다르게 동작해도 use case 코드는 한 벌이다 (§6·§7·§11 참조). 테스트 시 `InMemoryStateRepository`, `NoopAutoStart` 같은 가짜 adapter 를 주입해 빠르게 검증.
- **OS별 adapter 의 물리적 격리**: `platform-linux` · `platform-macos` · `platform-windows` 가 각각 별도 crate 라 **빌드 타겟에 해당하는 crate 만 컴파일**된다. Linux 서버에서 `platform-macos` 소스 오류가 CI 에 새지 않고, `unsafe` · syscall 바인딩도 크레이트 경계 안에 갇힌다.
- **Infra 카테고리 분리**: SQLite → Postgres, axum → 다른 서버 프레임워크로의 교체가 `infra-sqlite` · `infra-http` 만 손대면 된다. `infra-logging` 도 같은 이유로 분리 — Phase 3 의 JSON 로테이션·백프레셔 변경이 다른 crate 를 건드리지 않는다.
- **Server 배포 크기 최소화**: `cargo build -p my-supervisor-app-daemon` 은 `desktop` (Tauri, GTK/WebView2) 을 건드리지 않는다. 플랫폼별로도 `platform/<현재_OS>` 만 링크된다.
- **`shared` 와 `core` 의 역할 분리**: `shared` 는 네트워크·파일로 나가는 **wire format** 만 (DTO, 설정 파일 스키마). `core` 는 **도메인 모델** 과 **port trait**. 둘을 섞지 않아야 API 스펙이 도메인 변경에 휘둘리지 않고, 역으로 도메인이 wire 포맷 호환성에 묶이지 않는다.
- **Feature 단위 프론트엔드**: `packages/ui` 의 `features/*` 가 백엔드 port 분리와 대칭. 각 feature 폴더는 자기 UI + service 호출 + 훅을 자기 안에 둔다. 공용 shadcn 컴포넌트는 `components/ui`, HTTP/WS 클라이언트는 `services/` 에 분리.

## 4. 컴포넌트 설계

컴포넌트는 **(a) 실행 형태** (bin/lib) 와 **(b) 레이어 위치** 를 교차해서 본다. §3 의 레이어(core / application / shared+config / infra / platform / host)를 기준으로 정리한다.

### 4.1 Host — 런타임 조립과 실행 지점

실제로 빌드되는 최종 산출물. Hexagonal 의 "composition root" 역할만 한다. 도메인 로직은 들어가지 않는다.

#### 4.1.1 `daemon` (lib + bin `msv-daemon`)

공통 데몬 런타임. 라이브러리 타깃은 `core`/`application`/adapter 조립, 데이터 경로, 기본 loopback 주소를 제공한다. `msv-daemon` 바이너리는 이 런타임을 HTTP 서버로 띄우는 얇은 launcher 이다.

**책임:**
- `core` 의 도메인 타입과 port trait 를 임포트
- 현재 타겟 OS 에 맞는 `platform/*` adapter 를 `#[cfg(target_os = "...")]` 로 선택 → trait object 로 `application` 에 주입
- `infra/sqlite`, `infra/http`, `infra/logging`, `config` 를 조립
- 공통 런타임 진입점: `build_runtime()`으로 `OperationsFacade`와 Router를 생성
- 바이너리 진입점: tokio runtime 구성, 시그널 처리, graceful shutdown

**책임 경계:** 이 crate 에는 조립과 호스트 생명주기 이상의 도메인 로직이 들어가지 않아야 한다. 테스트는 `application` 에서 한다.

**주요 크레이트:**

| 역할 | 크레이트 |
|---|---|
| 비동기 런타임 | `tokio` |
| 시그널 | `signal-hook`, `signal-hook-tokio` |
| DI / 에러 | `anyhow`, `thiserror` |

`sqlx`, `axum`, `nix`, `windows` 등은 각 infra/platform crate 의 내부 의존이며 `daemon` 은 concrete adapter 를 선택해 연결만 한다.

#### 4.1.2 `cli` (bin `msv`)

데몬 HTTP API를 호출하는 얇은 클라이언트. `shared` crate 의 wire 타입을 재사용하므로 API 변경이 컴파일 타임에 잡힌다. 기본 접속 주소는 `daemon` 런타임의 상수를 사용한다.

**명령 체계:**

```
msv start <name>           # 프로세스 시작
msv stop <name>            # 중지
msv restart <name>         # 재시작
msv ps                     # 목록
msv logs <name> [-f]       # 로그 (follow 옵션)
msv add -c config.toml     # 설정 추가
msv remove <name>          # 제거
msv reload                 # 설정 리로드
msv daemon start|stop      # 데몬 자체 제어
msv daemon status
msv ui                     # 브라우저로 WebUI 열기
```

**품질 요구사항:**
- `-o json` 플래그로 스크립트 친화 출력
- 명확한 exit code 규약 (0=성공, 1=일반 실패, 2=프로세스 없음, 3=데몬 미실행)
- `logs -f`는 WebSocket 구독 기반

**크레이트:** `clap`, `reqwest`, `comfy-table`, `indicatif`, `serde_json`, `my-supervisor-shared`

#### 4.1.3 `desktop` (Tauri bin)

**GUI 가 있는 my-supervisor.** `daemon` 라이브러리의 런타임 조립을 **인프로세스로 재사용** 하고, Tauri invoke와 test-only devBridge를 제공한다. **별도 `msv-daemon` 프로세스를 spawn 하지 않는다**(DD-002).

**책임:**
- 코어 임베드 + 내장 axum HTTP/WS 서버 기동 (`127.0.0.1:<port>`)
- WebView 에 `http://127.0.0.1:<port>` 로드 (번들된 `packages/ui` 를 서빙)
- 트레이 아이콘 + 메뉴 (창을 닫아도 tray 로 상주 → supervision 유지)
- **macOS 자동화 권한(Accessibility / Input Monitoring) 보유 주체** — 윈도우·핫키 Rule 실행 (DD-027 · DD-029). 헤드리스 `daemon` 은 이 경로가 비활성
- 네이티브 알림 (crash loop 진입 등)
- OS 자동 시작 등록 (user-level) — `platform/*` 의 `AutoStartService` adapter 재사용 가능

**Tauri 플러그인:**
- `tauri-plugin-autostart` — 로그인 시 자동 시작 (간이 경로; 정식 OS 서비스 등록은 `platform/*` 사용)
- `tauri-plugin-single-instance` — 이중 실행 방지
- `tauri-plugin-notification` — 네이티브 알림
- `tauri-plugin-shell` — 브라우저 열기 등

**프론트엔드:** `packages/ui` 빌드 산출물을 Tauri WebView 와 외부 브라우저가 **동일하게** 로드. Tauri 의 `invoke` IPC 에 의존하지 않고 순수 HTTP/WebSocket 만 사용 → 같은 UI 가 Server 배포의 브라우저 접속에도 그대로 작동. 근거는 DD-016.

### 4.2 Application — Use Case 레이어

`apps/my_supervisor/application`. `core::ports` 의 trait 만 소비하는 순수 로직. 각 use case 는 한 파일·한 struct.

**Process 영역 use case:**

- `StartProcess` — spec 조회 → `LifecycleController::spawn_*` → `StateRepository::save_started`
- `StopProcess` — 상태 검증 → `ShutdownSignaler::request_graceful` → `StateRepository::save_stopped`
- `RestartProcess` — backoff 계산 + crash loop 감지 후 `Stop` → `Start` 체이닝 (OS 차이 없음)
- `ReconcileChildren` — 10초 주기. `LifecycleController::probe_alive` + 신원 3종 세트 (§10) 검증 → 사망/고아 판정
- `ReloadConfig` — `ConfigSource::load` → diff → 영향 받는 프로세스만 재시작
- `RegisterAutoStart` / `UnregisterAutoStart` — `AutoStartService` 호출

**Job 영역 use case** (§12 참조):

- `RegisterJob` / `UpdateJob` / `DeleteJob` — CRUD. 등록·수정 시 **순환 감지** (`cycle_detected`), `DeleteJob` 은 downstream 의존 존재 시 `has_dependents` 로 거부(→ `--force` 시 의존 해제 후 진행)
- `TriggerJobManual` — ad-hoc 실행. trigger 타입 무관하게 즉시 `JobRunner` 호출
- `ScheduleTick` — `Scheduler` 가 cron/interval/one-shot 이벤트를 발행하면 수신 → `on_overlap` 정책(`Skip` / `Queue` / `Parallel`) 적용 후 `JobRunner::run`
- `ObserveJobRunCompleted` — 완료 시 의존 Job (downstream) 순회. `on_dependency_failure` 에 따라 skip/run 결정
- `CancelJobRun` — 진행 중 Run 중단. 내부적으로 `ShutdownSignaler` 사용

**Rule 영역 use case** (§13 참조):

- `RegisterRule` / `UpdateRule` / `DeleteRule` — CRUD. 등록 시 트리거·액션 유효성 검증. **윈도우·핫키 액션이 포함됐는데 호스트가 비-macOS(자동화 seam 미주입)면 `not supported on this platform` 으로 거부** (DD-027)
- `TriggerRuleManual` — 트리거 무관하게 액션을 즉시 1회 실행 (Rules 탭의 시험 발화 / `msv rule run <name>`)
- `ObserveEvent` — `EventSource`(파일/폴더·타이머) 또는 macOS 이벤트 seam 이 이벤트를 발행하면 수신 → 매칭 Rule 의 액션 실행. 액션은 `LifecycleController`(프로세스)·`JobRunner`(Job)·`WindowAutomation`(윈도우) 등 기존/신규 port 로 위임

Use case 는 OS 를 모른다 (#[cfg] 없음). 모든 플랫폼 차이는 port 호출로 위임한다 — **시간 트리거는 `Scheduler`(Job), 이벤트 트리거는 `EventSource`(Rule)** 로 분리되며(DD-028), 윈도우·핫키 자동화는 `Option<&dyn WindowAutomation>` 같은 **단일 구현 seam** 으로 소비한다(macOS 외 호스트에는 `None`, DD-027). spawn 은 `LifecycleController` 에 위임.

### 4.3 Core — 도메인 + Ports

`apps/my_supervisor/core`. 프로젝트의 중심. 다른 워크스페이스 crate 에 **의존하지 않는다**.

**`core::domain`** — 값 객체와 엔티티.

```rust
// apps/my_supervisor/core/src/domain/process.rs 발췌
pub struct ProcessSpec {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub management_mode: ManagementMode, // Direct | SystemRegistered
    pub lifecycle: LifecycleMode,        // Tied | Detached (Direct 모드에서만 의미)
    pub restart: RestartPolicy,          // SystemRegistered 시 OS 에 위임, 본 policy 는 비활성
    pub shutdown: ShutdownPolicy,
    /* … */
}

pub enum ManagementMode {
    /// 데몬이 직접 spawn·감시·재시작. 기본값.
    Direct,
    /// OS 서비스 매니저에 등록하고 제어·상태는 OS 경유.
    /// unit_name 은 systemd unit / launchd label / Windows Service 이름으로 매핑.
    SystemRegistered { unit_name: String },
}

pub enum ProcessState { Starting, Running, Stopping, Crashed, Stopped }

pub struct ChildHandle {
    pub process_id: Uuid,       // MYSUPERVISOR_PROCESS_ID (Direct 모드)
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    /* opaque OS 핸들은 platform crate 가 소유. SystemRegistered 에서는 unit_name 이 주 식별자. */
}
```

Job 영역의 도메인 타입은 §12 에 상세. 요약:

```rust
// apps/my_supervisor/core/src/domain/job.rs 발췌
pub struct Job {
    pub id: JobId,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub trigger: JobTrigger,                            // one-of
    pub on_overlap: OverlapPolicy,                      // Skip | Queue | Parallel
    pub on_dependency_failure: DependencyFailurePolicy, // Skip | RunAnyway
    pub timeout: Option<Duration>,
    pub log_retention: LogRetention,
}

pub enum JobTrigger {
    Cron(String),               // "0 */6 * * *"
    Interval(Duration),         // every 5m
    OneShot(DateTime<Utc>),
    DependsOn(Vec<JobId>),      // AND 시맨틱, on-success 기본
}

pub enum JobRunState { Pending, Running, Succeeded, Failed, Cancelled, Skipped }
```

Rule(자동화) 영역의 도메인 타입은 §13 에 상세. 요약:

```rust
// apps/my_supervisor/core/src/domain/rule.rs 발췌
pub struct Rule {
    pub id: RuleId,
    pub name: String,
    pub trigger: RuleTrigger,        // one-of (이벤트 기반 — Job 의 시간 기반과 분리, DD-028)
    pub actions: Vec<RuleAction>,    // 순차 실행
    pub enabled: bool,               // 비활성 시 트리거 무시 (권한 미부여 윈도우/핫키 Rule 포함, DD-029)
}

pub enum RuleTrigger {
    FileChange { path: PathBuf, kind: FsEventKind }, // 크로스플랫폼 (EventSource)
    SystemEvent(SystemEventKind),  // USB·WiFi·화면잠금·앱 실행 — macOS 전용 seam (DD-027)
    Hotkey(HotkeySpec),            // 글로벌 핫키 — macOS 전용 seam (DD-027)
}

pub enum RuleAction {
    StartProcess(String), StopProcess(String),      // 운영 기둥 연계
    TriggerJob(JobId),                               // Job 수동 트리거
    RunCommand { command: String, args: Vec<String> },
    Window(WindowActionSpec),      // 이동·리사이즈·레이아웃 — macOS 전용 seam (DD-027)
}

pub enum RuleFireState { Fired, ActionsSucceeded, ActionsFailed, Skipped }
```

**`core::ports`** — trait 목록.

| Port trait | 역할 | adapter 위치 |
|---|---|---|
| `LifecycleController` | tied/detached spawn, process group 제어, alive 확인 (**Direct 모드 전용**) | `platform/*` |
| `ShutdownSignaler` | graceful → force kill 시퀀스 (**Direct 모드 전용**) | `platform/*` |
| `AutoStartService` | **데몬 자체** 의 OS 자동 시작 등록/해제 | `platform/*` |
| `ProcessServiceRegistrar` | **SystemRegistered 모드 전용**. 개별 프로세스를 OS 서비스 매니저에 register/unregister, start/stop/query_status/tail_logs | `platform/*` |
| `StateRepository` | 프로세스·히스토리·메트릭 영속화 | `infra/sqlite` (+ 테스트용 in-memory) |
| `JobRepository` | Job 정의 + JobRun 이력 영속. 등록 시 순환 감지 포함 | `infra/sqlite` 확장 |
| `Scheduler` | cron/interval/one-shot 트리거를 평가해 "실행 시점 이벤트" 를 application 으로 발행. 의존성 트리거는 `JobRepository` run 이벤트 구독으로 전파 | `infra/scheduler` |
| `JobRunner` | Job 을 transient 프로세스로 spawn → stdout/stderr 수집 → exit 대기 → JobRun 확정. 내부적으로 `LifecycleController::spawn_detached` 재사용 | `application` 조합 — 신규 crate 불필요 |
| `LogSink` | 자식 stdout/stderr 스트림 수집·로테이션 (+ `for_job_run(JobRunId)` 경로) | `infra/logging` |
| `HttpServer` | HTTP/WS 엔드포인트 호스팅 | `infra/http` |
| `ConfigSource` | TOML 로드·검증·watch | `config` |
| `HealthChecker` | HTTP / TCP / exec 헬스체크 | `infra/*` (Phase 3) |
| `SystemClock` | 시간 의존 (테스트 가짜 주입용) | `infra/*` |
| `EventSource` | **Rule 이벤트 트리거** — 파일/폴더 watch·타이머. 시간 기반 `Scheduler`(Job)와 분리 (DD-028) | `infra/*` (`notify` 기반, **크로스플랫폼**) |
| `WindowAutomation` ✱ | Rule 의 윈도우 액션 (이동·리사이즈·레이아웃) | `platform/macos` (AX API) |
| `HotkeyBinder` ✱ | Rule 의 글로벌 핫키 트리거 | `platform/macos` (event tap) |
| `SystemEventWatcher` ✱ | Rule 의 macOS 시스템 이벤트 (USB·WiFi·화면잠금·앱 실행) | `platform/macos` (NSWorkspace/IOKit) |

> ✱ **단일 구현 seam** — trait 는 `core::ports` 에 있으나 구현자가 `platform/macos` **하나뿐이며 Linux/Windows stub 을 만들지 않는다.** 여러 OS 가 구현하는 일반 port 와 달리, `application` 은 `Option<&dyn …>` 로 소비하고 `desktop`(macOS)만 `Some` 을 주입한다(비-macOS 호스트는 `None`). 근거·미러링 함정 회피: DD-027.

### 4.4 Shared — Wire Types

`apps/my_supervisor/shared`. HTTP/WS DTO 와 TOML 설정 스키마만 담는다. `core` 와 분리된 이유: wire format 호환성 제약(`#[serde(rename_all = "snake_case")]` 같은 어노테이션)이 도메인 타입으로 누출되지 않게 하기 위함.

```rust
// apps/my_supervisor/shared/src/api.rs 예시
#[derive(Serialize, Deserialize)]
pub struct ProcessStatusDto {
    pub name: String,
    pub state: ProcessStateDto,
    pub pid: Option<u32>,
    pub restart_count: u32,
    pub started_at: Option<DateTime<Utc>>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}
```

`infra/http` 와 `cli` 가 같은 DTO 를 임포트하기 때문에 API 변경이 양쪽에 동시 반영된다 (DD-003 의 "단일 통신 프로토콜" 을 타입 레벨에서 강제).

### 4.5 Config

`apps/my_supervisor/config`. `ConfigSource` port 의 참조 구현. `shared::config` 스키마를 사용하여 TOML 을 파싱하고, `notify` 크레이트로 파일 변경을 감지한다. `core` 외에는 의존하지 않기 때문에 테스트에서 `InMemoryConfigSource` 와 교체 가능.

### 4.6 Infrastructure Adapters

`apps/my_supervisor/infra/*`. 기술 스택에 종속적인 port 구현.

| Crate | 구현 port | 주요 의존 |
|---|---|---|
| `infra/sqlite` | `StateRepository`, `JobRepository` | `sqlx` (SQLite, WAL 모드) |
| `infra/http` | `HttpServer` (+ WS hub) | `axum`, `tower`, `tokio-tungstenite`, `my-supervisor-shared` |
| `infra/logging` | `LogSink` + 로테이션 (run 단위 로그 아카이브 경로 포함) | `tracing`, `tracing-subscriber`, `file-rotate` |
| `infra/scheduler` | `Scheduler` — cron 5-field / interval / one-shot / 의존성 타이머 평가 | `tokio-cron-scheduler` (또는 `cron` + 자체 `tokio::time`) |

교체 가능성: SQLite → Postgres 시 `infra/postgres` 를 새로 만들고 `daemon` 의 DI 만 교체하면 된다. `core` · `application` 은 무변경. `infra/scheduler` 도 동일 — 다른 스케줄러 라이브러리로 교체 시 이 crate 만 재작성.

### 4.7 Platform Adapters

`apps/my_supervisor/platform/*`. OS 에 종속적인 port 구현. 각 crate 는 **자기 OS 에서만 컴파일**되도록 `[target]` 블록 또는 `#[cfg]` 로 가드한다.

| Crate | 대표 구현 | OS-native API |
|---|---|---|
| `platform/linux` | `LinuxLifecycle`, `UnixShutdown`, `SystemdUserUnit` (데몬용), `SystemdUserUnitProcess` (프로세스용), `JournaldLogSink`(선택) | `prctl`(`nix`), `signal-hook`, `systemctl --user`, `systemd` + `tracing-journald` |
| `platform/macos` | `MacLifecycle`, `UnixShutdown`, `LaunchdAgent` (데몬용), `LaunchdAgentProcess` (프로세스용) | `kqueue`, `launchctl bootstrap/bootout`, `plist` |
| `platform/windows` | `WindowsLifecycle`, `WindowsShutdown`, `TaskSchedulerEntry`/`WindowsService` (데몬용), `WindowsServiceProcess` (프로세스용), `EventLogSink`(선택) | Job Object, `CTRL_BREAK_EVENT`, `sc create/delete/start/stop`, Event Log (`windows` crate) |

`AutoStartService` 와 `ProcessServiceRegistrar` 는 **서로 다른 port** 지만 같은 OS 서비스 매니저를 쓴다. 둘의 차이:

- `AutoStartService` — **데몬 자체** 를 부팅 시 기동하기 위한 단일 unit (예: `my-supervisor.service`)
- `ProcessServiceRegistrar` — **관리 대상 프로세스마다** 하나씩의 unit (예: `my-supervisor-managed-<name>.service`). 이름 네임스페이스로 충돌 방지.

세 crate 가 같은 port trait (`LifecycleController` 등) 을 구현하기 때문에 `daemon` 에서는:

```rust
#[cfg(target_os = "linux")]
let lifecycle: Arc<dyn LifecycleController> =
    Arc::new(my_supervisor_platform_linux::LinuxLifecycle::new());
#[cfg(target_os = "macos")]
let lifecycle: Arc<dyn LifecycleController> =
    Arc::new(my_supervisor_platform_macos::MacLifecycle::new());
#[cfg(target_os = "windows")]
let lifecycle: Arc<dyn LifecycleController> =
    Arc::new(my_supervisor_platform_windows::WindowsLifecycle::new());
```

이후 `application` 레이어는 `Arc<dyn LifecycleController>` 만 보고 동작 → OS 분기가 application 코드에는 전혀 등장하지 않는다.

### 4.8 Frontend (`packages/ui`)

React + Vite 기반 단일 번들. feature 단위로 모듈화한다 (DD-020).

```
packages/ui/src/
├── features/
│   ├── processes/    # [운영] 목록 / 상세 / 시작·중지·재시작 / 설정 폼
│   ├── jobs/         # [운영] Jobs 목록 / 상세(Overview·Runs·Config·Dependencies) / Run 상세 / 수동 트리거
│   ├── logs/         # [운영] 로그 뷰어 + follow
│   ├── rules/        # [자동화] Rule 목록 / 상세(트리거·액션) / 시험 발화 / 권한 상태(macOS)
│   ├── daemon/       # [공통] 데몬 상태·리로드·종료
│   └── settings/     # [공통] 전역 설정 편집
├── components/ui/    # shadcn 공용 컴포넌트 (CSS 변수 토큰 기반)
├── services/         # api.ts (REST), events.ts (WS)
└── shared/           # 훅·유틸·타입 (shared crate 의 API 타입을 TS 로 매핑)
```

각 feature 폴더는 자기 UI 컴포넌트, 자기 service 호출, 자기 훅을 가진다. 크로스 feature 공유가 필요하면 `shared/` 로 승격한다. Tauri 의 `invoke` IPC 에는 의존하지 않으므로 번들은 Tauri 내부·외부 브라우저에서 동일하게 작동.

**상위 내비게이션 IA (동등 이원, DD-026)**: 좌측 내비를 두 기둥 + 공통으로 그룹화한다.

- **운영(Operations)**: `Processes · Jobs · Logs`
- **자동화(Automation)**: `Rules` (이벤트→액션; 윈도우·핫키 포함. 윈도우·핫키 등 macOS 전용 기능은 비-macOS 호스트에서 비활성 + 안내)
- **공통(Common)**: `Daemon · Settings`

Jobs 탭은 MVP 부터 별도 존재(DD-022, DD-023), Rules 탭은 자동화 기둥의 진입점(DD-026)이다.

**테마**: **다크 모드와 라이트 모드를 동등 지원** (DD-021). shadcn/ui 의 CSS 변수 규약 (`--background`, `--foreground`, `--primary`, `--muted`, `--destructive` 등) 을 토큰 단일 출처로 사용하고, 두 모드 모두 `:root` / `[data-theme="dark"]` 에 대칭 정의. `Settings → Theme` 의 `auto` 값은 OS `prefers-color-scheme` 을 추종한다. 디자인 시안 승인 절차에서는 **두 모드 스크린샷을 동시 확인**해야 한다 — 한쪽만 검토하고 다른 쪽을 나중에 맞추면 토큰 대신 하드코드 색이 스며드는 비용이 커진다.

## 5. 통신 프로토콜

### 5.1 HTTP API

RESTful. 엔드포인트 예시:

```
GET    /api/v1/processes              # 목록
POST   /api/v1/processes              # 추가 (body: ProcessConfig)
GET    /api/v1/processes/{name}       # 상세
DELETE /api/v1/processes/{name}       # 제거
POST   /api/v1/processes/{name}/start
POST   /api/v1/processes/{name}/stop
POST   /api/v1/processes/{name}/restart
GET    /api/v1/processes/{name}/logs?tail=100&since=<timestamp>

GET    /api/v1/daemon/status
POST   /api/v1/daemon/reload
POST   /api/v1/daemon/shutdown
```

### 5.2 WebSocket

실시간 이벤트 구독.

```
WS /api/v1/events                     # 전역 이벤트 (상태 변경 등)
WS /api/v1/processes/{name}/logs      # 특정 프로세스 로그 follow
```

**이벤트 타입:**
- `process.state_changed`
- `process.crashed`
- `process.crash_loop_detected`
- `process.health_check_failed`

### 5.3 바인딩 제약

- 기본 및 유일 바인딩: `127.0.0.1:9876` *(포트는 설정 가능)*
- `0.0.0.0` 바인딩은 코드 레벨에서 금지 (보안)
- 추가 옵션: Unix domain socket (`~/.local/state/my-supervisor/daemon.sock`) — CLI↔데몬 고속/안전 통신용

### 5.4 오류 응답 포맷

모든 API 오류는 동일한 JSON 구조로 응답한다.

```json
{
  "error": {
    "code": "process_not_found",
    "message": "Process 'api-server' is not registered",
    "details": { "name": "api-server" }
  }
}
```

| 필드 | 설명 |
|---|---|
| `error.code` | 기계 판독용 안정 코드 (snake_case, 변경 시 breaking change) |
| `error.message` | 사람 판독용 메시지 (영문 기준, 로그 수집에 적합) |
| `error.details` | 선택 필드. 컨텍스트 정보(프로세스 이름, 상태 등) |

**HTTP 상태 코드 규약:**

| 상태 코드 | 용도 | 대표 `code` |
|---|---|---|
| `400 Bad Request` | 잘못된 요청 바디·파라미터 | `invalid_request`, `invalid_config` |
| `404 Not Found` | 프로세스·리소스 없음 | `process_not_found` |
| `409 Conflict` | 현재 상태와 충돌 (예: 이미 실행 중) | `already_running`, `crash_loop_detected` |
| `500 Internal Server Error` | 데몬 내부 오류 | `internal_error`, `spawn_failed` |

WebSocket 채널은 연결 해제 시 close frame의 reason에 같은 `code`를 담는다.

세부 레퍼런스는 `API.md` 참조.

## 6. 프로세스 생명주기

### 6.1 생명주기 모드 (옵셔널)

프로세스별로 설정 가능.

| 모드 | 의미 | 사용 예 |
|---|---|---|
| `tied` *(기본)* | 데몬 종료 시 자식도 종료 | 개발 서버, 테스트 환경 |
| `detached` | 자식이 데몬과 독립적으로 생존 | 장기 실행 서비스 |

### 6.2 Port: `LifecycleController`

생명주기 제어는 **OS별로 사용하는 커널 기능이 완전히 다르다**. 같은 "tied 모드"여도 Linux 는 `PR_SET_PDEATHSIG`, Windows 는 Job Object 의 `KILL_ON_JOB_CLOSE`, macOS 는 shutdown hook 이라는 식. 이 차이를 `application` 레이어가 알면 코드가 3중 분기투성이가 된다. 따라서 단일 port trait 로 묶고 OS crate 가 각자 구현한다.

```rust
// apps/my_supervisor/core/src/ports/lifecycle.rs
#[async_trait]
pub trait LifecycleController: Send + Sync {
    async fn spawn_tied(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError>;
    async fn spawn_detached(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError>;
    async fn probe_alive(&self, handle: &ChildHandle) -> Result<Aliveness, ProbeError>;
    async fn reap_on_shutdown(&self, handles: &[ChildHandle]) -> Result<(), ReapError>;
}
```

각 OS crate 의 구현 전략:

| OS Crate | Tied 구현 | Detached 구현 | 부가 기능 |
|---|---|---|---|
| `platform/linux` (`LinuxLifecycle`) | `pre_exec` 훅에서 `prctl(PR_SET_PDEATHSIG, SIGTERM)` | `setsid()` 새 세션 | `prctl(PR_SET_CHILD_SUBREAPER, 1)` — 데몬을 subreaper 로 지정, double-fork 손자도 init 대신 데몬으로 reparent |
| `platform/windows` (`WindowsLifecycle`) | Job Object 생성 → `SetInformationJobObject` 에 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` → 자식 할당 | `DETACHED_PROCESS` 플래그 + Job Object breakaway 허용 | Job Object 가 Linux 의 subreaper 와 동일한 역할을 자연스럽게 수행 |
| `platform/macos` (`MacLifecycle`) | `PR_SET_PDEATHSIG` 없음 → 데몬 shutdown hook 에서 전체 자식 정리 + 자식이 `kqueue` 로 부모 PID 감시해 자발 종료 옵션 | `setsid()` (Unix 공통) | reconciliation 루프(§10)가 subreaper 부재를 보완 |

`application::StartProcess` 는 위 표의 어느 항목도 직접 알지 않는다. 또한 `management_mode` 에 따라 `LifecycleController` (Direct) 또는 `ProcessServiceRegistrar` (SystemRegistered) 로 분기한다 (§6.4 참조):

```rust
// apps/my_supervisor/application/src/start_process.rs 발췌
pub async fn start_process(
    spec: &ProcessSpec,
    lifecycle: &dyn LifecycleController,
    registrar: &dyn ProcessServiceRegistrar,
    repo: &dyn StateRepository,
) -> Result<ProcessState, StartError> {
    match &spec.management_mode {
        ManagementMode::Direct => {
            let handle = match spec.lifecycle {
                LifecycleMode::Tied     => lifecycle.spawn_tied(spec).await?,
                LifecycleMode::Detached => lifecycle.spawn_detached(spec).await?,
            };
            repo.save_started_direct(&handle).await?;
            Ok(ProcessState::Running)
        }
        ManagementMode::SystemRegistered { unit_name } => {
            registrar.start(unit_name).await?;
            repo.save_started_system(unit_name).await?;
            Ok(ProcessState::Running)
        }
    }
}
```

### 6.3 시작 시퀀스 (Direct 모드)

1. 설정에서 프로세스 정의 로드
2. 작업 디렉터리 확인
3. 환경 변수 구성 (시스템 환경 + 사용자 정의 + `MYSUPERVISOR_PROCESS_ID=<uuid>` 태그)
4. Unix: `pre_exec`에서 `prctl` 설정 → `setsid`/`setpgid`
5. Windows: Job Object 생성 → 프로세스 spawn (suspended) → Job에 할당 → resume
6. stdout/stderr 파이프를 비동기 reader로 연결
7. SQLite에 시작 기록 (pid, start_time, uuid, command hash)

### 6.4 관리 모드 분기 — Direct vs SystemRegistered

각 Process 는 `management_mode` 로 두 모드 중 하나를 선택한다 (DD-025). 아래 표가 모드별 동작 차이를 요약한다. `application` 레이어는 `management_mode` 로 분기하지만, **OS별 분기는 여전히 각 port 의 adapter 내부에만** 존재한다.

| 항목 | **Direct** *(기본)* | **SystemRegistered** |
|---|---|---|
| spawn 경로 | `LifecycleController::spawn_tied` / `spawn_detached` | `ProcessServiceRegistrar::register` → `start` |
| 상태 조회 | `LifecycleController::probe_alive` + 신원 3종 세트 (§10) | `ProcessServiceRegistrar::query_status` (내부적으로 `systemctl show` / `launchctl list` / `sc query`) |
| 종료 | `ShutdownSignaler::request_graceful` → `force_kill` | `ProcessServiceRegistrar::stop` (OS 가 자체 graceful 수행) |
| **재시작** | `application::RestartProcess` — backoff + crash loop 감지 | `RestartProcess` 는 **no-op + 사용자 안내** 반환. OS 의 `Restart=on-failure` 같은 지시어가 재시작 담당 |
| 로그 수집 | `LogSink` 의 stdout/stderr 파이프 캡처 | `LogSink` 의 OS-native 읽기 경로 — journald (Linux) / `~/Library/Logs` (macOS) / Event Log (Windows) |
| 신원 식별 | `MYSUPERVISOR_PROCESS_ID` UUID 태그 + PID + start_time | `unit_name` (예: `my-supervisor-managed-api.service`) |
| 데몬 재시작 시 | reconciliation 루프 (§10) 로 입양 | OS 가 이미 살아있을 수 있음 — `query_status` 로 동기화만 |
| 데몬 크래시 영향 | `tied` 이면 자식도 죽음 / `detached` 이면 고아 | **무관** — OS 가 계속 관리 |
| 등록·제거 비용 | 즉시 | unit 파일 생성·삭제 + `systemctl daemon-reload` 같은 부수효과 |

**재시작 엔진 충돌 회피 규칙 (엄수)**: `management_mode = SystemRegistered` 인 프로세스의 `ProcessConfig.restart` 는 OS unit 파일 생성 시 `Restart=` 지시어로 **변환되어 적용**된다. 데몬 내부의 `RestartProcess` use case 는 이 모드에서 호출돼도 실제 재시작을 수행하지 않고 `Ok(Noop { reason: "managed by system" })` 반환. 이것이 문서화되지 않으면 "왜 재시작이 두 번 시도되는가" 같은 버그가 생긴다 (DD-025 근거).

**Direct ↔ SystemRegistered 전환**: 런타임에 가능 (`POST /api/v1/processes/{name}/convert`). 전환 순서:

1. 현재 실행 중이면 현재 모드로 stop
2. 현재 모드의 흔적 정리 (Direct → 상태 테이블 정리 / System → `unit_name` unregister + 파일 삭제)
3. 새 모드로 register 또는 설정만 반영
4. `ProcessSpec.management_mode` 업데이트 및 저장
5. 요청에 `auto_start: true` 가 있으면 새 모드로 start

전환 실패 시 **원래 모드 유지** (atomic 보장). 실패 원인은 API 에러 응답으로 반환.

## 7. 시그널 처리

### 7.1 Port: `ShutdownSignaler`

자식 프로세스로 "정지하라" 는 신호를 보내는 방식은 OS 마다 다르지만, `application` 레이어에는 "정상 요청 → grace period → 강제 종료" 라는 동일한 논리가 있다. 이를 port 로 추상화한다.

```rust
// apps/my_supervisor/core/src/ports/shutdown.rs
#[async_trait]
pub trait ShutdownSignaler: Send + Sync {
    async fn request_graceful(&self, target: &ChildHandle, cfg: &ShutdownPolicy)
        -> Result<(), SignalError>;
    async fn force_kill(&self, target: &ChildHandle) -> Result<(), SignalError>;
}
```

각 OS crate 의 구현 (자식 프로세스 대상):

**Unix (`platform/linux`, `platform/macos` — 공통 `UnixShutdown`):**

1. `SIGTERM` (설정 가능) 송신
2. `grace_period` 대기 (프로세스별, 기본 10초)
3. 미종료 시 `SIGKILL`
4. process group 전체 대상: `kill(-pgid, SIGTERM)` — 손자까지 포함

**Windows (`platform/windows` — `WindowsShutdown`):**

1. `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pgid)` — 콘솔 앱
2. 콘솔 없으면 `WM_CLOSE` 윈도우 메시지 또는 Named pipe 합의 프로토콜
3. 최후: `TerminateJobObject` 또는 `TerminateProcess`

`application::StopProcess` 는 단순히 `signaler.request_graceful(...)` 를 호출할 뿐, 위 분기를 전혀 모른다.

### 7.2 데몬 자체의 시그널 처리

| 시그널 | 동작 |
|---|---|
| `SIGTERM` / `SIGINT` | 정상 종료 시작. 모든 자식 graceful shutdown 후 상태 저장·종료 |
| `SIGHUP` | 설정 리로드. 프로세스는 종료하지 않음 |
| `SIGUSR1` | 로그 파일 재오픈 (logrotate 외부 연동용) |
| `SIGUSR2` | 예약 (zero-downtime reload 후보) |
| `SIGCHLD` | 자식 종료 이벤트 (tokio가 내부 처리) |
| `SIGPIPE` | `SIG_IGN`으로 무시 (크래시 방지) |

**공통 원칙:**
- 모든 graceful 처리에 타임아웃 존재. 무한 대기 금지.
- Shutdown 진행 중에는 재시작 정책 **비활성화**.
- Shutdown 진행 상황을 로그로 남김.

**Windows 서비스 모드:**
- `SERVICE_CONTROL_STOP` 핸들러에서 동일한 graceful shutdown 수행.

## 8. 재시작 정책

재시작 정책 자체는 **OS 공통 로직**이다. `application::RestartProcess` use case 가 backoff 계산과 crash loop 감지를 수행하고, 실제 spawn/kill 은 `LifecycleController` · `ShutdownSignaler` port 를 호출한다. 따라서 이 섹션에는 `#[cfg(target_os)]` 분기가 없다.

**주의**: 본 §8 전체는 `management_mode = Direct` 프로세스에만 적용된다. `SystemRegistered` 프로세스는 재시작을 OS 서비스 매니저가 담당하며 (systemd `Restart=`, launchd `KeepAlive`, Windows Service `FailureActions`), `ProcessConfig.restart` 값은 unit 파일 생성 시 그쪽 지시어로 변환되어 적용된다 (§6.4, DD-025).

### 8.1 정책 종류

- `always` — 종료 사유 무관 재시작
- `on-failure` *(기본)* — 비정상 종료(exit != 0)일 때만
- `never` — 재시작하지 않음

### 8.2 Exponential Backoff

```
delay(n) = min(backoff_max_ms, backoff_initial_ms × backoff_multiplier^(n-1))
```

기본값 예시: `initial=1000ms`, `max=60000ms`, `multiplier=2.0`

### 8.3 Crash Loop 감지

슬라이딩 윈도우 기반:
- `crash_loop_window_sec` 내에 `crash_loop_threshold`번 이상 크래시 → crash loop 판정
- 판정 시 재시작 일시 중단, 알림 발송, `Crashed` 상태로 고정
- 사용자 수동 개입 필요 (UI·CLI의 `restart` 명령으로 카운터 리셋)

## 9. 로깅

### 9.1 파이프라인

```
자식 프로세스 stdout/stderr
        ↓ (tokio 비동기 pipe)
LineCodec 스트림
        ↓
bounded mpsc channel (process별, 예: 10k)
        ↓
tracing 이벤트 + 구조화 필드 (process, pid, stream)
        ↓
┌───────────┴───────────┐
│                       │
JSON 파일 (file-rotate)  WebSocket 구독자 (follow 중인 경우)
```

### 9.2 백프레셔

- bounded channel이 가득 차면 **가장 오래된 라인 drop**, `dropped_count` 메트릭 증가
- 자식 프로세스가 블록되지 않도록 pipe는 **항상 drain**
- WebSocket 측은 rate limit (초당 최대 라인). 초과 시 `... N lines dropped` 삽입

### 9.3 로테이션

- 크기 기반 (`max_file_size_mb`) + 파일 수 제한 (`max_files`)
- 시간 기반 (daily/hourly) 선택 가능
- gzip 압축 옵션
- 외부 logrotate와 연동하려면 `SIGUSR1`로 파일 재오픈

### 9.4 포맷

```json
{
  "timestamp": "2026-04-21T10:30:45.123Z",
  "process": "api-server",
  "pid": 12345,
  "stream": "stdout",
  "line": "Server listening on :3000"
}
```

## 10. 좀비·고아 프로세스 처리

### 10.1 신원 확인 3종 세트

PID만으로 자식을 식별하면 PID 재사용 때문에 위험. 다음을 조합:

1. **PID**
2. **프로세스 시작 시각** — `/proc/<pid>/stat` 22번 필드 또는 `sysinfo::Process::start_time()`
3. **UUID 환경 변수 태그** — spawn 시 `MYSUPERVISOR_PROCESS_ID=<uuid>` 주입, `/proc/<pid>/environ` (Linux) / `libproc` (macOS) / Toolhelp (Windows) 로 검증

세 가지 모두 일치해야 "내 자식"으로 인정.

### 10.2 Subreaper 기반 회수 (Linux)

`prctl(PR_SET_CHILD_SUBREAPER, 1)`로 데몬을 subreaper로 선언. 자식이 double-fork로 도망쳐도 손자 프로세스는 init(PID 1) 대신 데몬에게 reparent되어 회수 가능.

Windows는 Job Object가 같은 역할 수행.

### 10.3 Reconciliation 루프

10초 간격:

1. 관리 목록의 각 프로세스 실재 확인 (신원 3종 세트로)
2. 신원 불일치 → 사망 처리 후 재시작 정책 적용
3. Unix: `waitpid(-1, WNOHANG)`으로 좀비 즉시 수거
4. 예상 목록에 없으나 내 `MYSUPERVISOR_PROCESS_ID` UUID 태그가 붙은 프로세스 발견 → 고아. 정책에 따라 입양 또는 종료.

### 10.4 Write-Ahead State

데몬은 상태 변경 **전에** SQLite에 기록. 비정상 종료 후 재시작해도 마지막으로 알려진 자식 목록을 복원 가능.

## 11. 시스템 서비스 통합

OS 서비스 매니저(systemd user unit / launchd agent / Windows Service) 와의 연동은 **두 가지 독립적인 결정**으로 구성된다. 둘을 혼동하면 설계가 꼬인다:

- **(A) 데몬 자체의 기동 모드** — my-supervisor 데몬 자체를 부팅 시 OS 서비스로 자동 기동할지 여부 → `AutoStartService` port. **Opt-in**, 기본 off.
- **(B) 관리 대상 프로세스의 관리 모드** — 각 Process 를 데몬이 직접 관리(Direct) 할지, OS 서비스 매니저에 맡길(SystemRegistered) 지 → `ProcessServiceRegistrar` port. **프로세스별 선택**, 기본 Direct.

두 축은 독립적으로 조합된다. 예: (A off, B Direct) = 기본. (A on, B Direct) = 데몬은 부팅 시 기동되지만 자식은 직접 관리. (A on, B SystemRegistered) = 데몬과 자식 모두 OS 가 관리. (A off, B SystemRegistered) = 흔치 않지만 가능 (데몬은 수동 기동, 자식은 OS 등록).

### 11.1 (A) 데몬 자체 기동 모드

데몬 자체를 OS 서비스로 등록할지는 사용자가 UI/CLI 토글로 켠다. 기본은 off.

#### Port: `AutoStartService`

```rust
// apps/my_supervisor/core/src/ports/autostart.rs
#[async_trait]
pub trait AutoStartService: Send + Sync {
    async fn enable(&self, scope: AutoStartScope) -> Result<(), AutoStartError>;
    async fn disable(&self) -> Result<(), AutoStartError>;
    async fn status(&self) -> Result<AutoStartStatus, AutoStartError>;
}

pub enum AutoStartScope { User, System } // user-level vs 시스템 전역
```

#### OS별 adapter

| OS Crate | 구현체 | 파일 경로 | 제어 |
|---|---|---|---|
| `platform/linux` | `SystemdUserUnit` | `~/.config/systemd/user/my-supervisor.service` | `systemctl --user enable/start my-supervisor` |
| `platform/macos` | `LaunchdAgent` | `~/Library/LaunchAgents/com.my-supervisor.daemon.plist` | `launchctl bootstrap gui/$(id -u) ...` |
| `platform/windows` | `TaskSchedulerEntry` 또는 `WindowsService` | Task Scheduler XML · `sc create my-supervisor` | `sc start/stop my-supervisor` |

UI/CLI 는 `AutoStartService` trait 만 호출 → OS 분기는 `daemon` DI 지점에만 존재.

### 11.2 (B) 관리 대상 프로세스 관리 모드

각 Process 는 `management_mode = Direct | SystemRegistered` 를 선택한다 (§6.4, DD-025). SystemRegistered 선택 시 해당 프로세스는 OS 서비스로 등록되고 이후 제어는 `ProcessServiceRegistrar` port 를 통해 이뤄진다.

#### Port: `ProcessServiceRegistrar`

```rust
// apps/my_supervisor/core/src/ports/process_service.rs
#[async_trait]
pub trait ProcessServiceRegistrar: Send + Sync {
    /// unit 파일 생성 + register (daemon-reload 등 부수 절차 포함).
    /// ProcessConfig.restart 는 OS native 지시어로 변환되어 unit 에 주입된다.
    async fn register(&self, spec: &ProcessSpec) -> Result<(), RegistrarError>;
    /// unit 해제 + 파일 삭제. 실패 시 잔재 정리용 `repair` 경로 제공.
    async fn unregister(&self, unit_name: &str) -> Result<(), RegistrarError>;
    async fn start(&self, unit_name: &str) -> Result<(), RegistrarError>;
    async fn stop(&self, unit_name: &str) -> Result<(), RegistrarError>;
    async fn query_status(&self, unit_name: &str) -> Result<ProcessState, RegistrarError>;
    /// OS-native 로그 경로에서 최근 N 줄 읽기 (journald / launchd logs / Event Log).
    async fn tail_logs(&self, unit_name: &str, tail: usize) -> Result<Vec<LogLine>, RegistrarError>;
}
```

#### OS별 adapter

| OS Crate | 구현체 | unit 파일 경로 | 제어 |
|---|---|---|---|
| `platform/linux` | `SystemdUserUnitProcess` | `~/.config/systemd/user/my-supervisor-managed-<name>.service` | `systemctl --user start/stop/status …` |
| `platform/macos` | `LaunchdAgentProcess` | `~/Library/LaunchAgents/com.my-supervisor.managed.<name>.plist` | `launchctl bootstrap/bootout`, `kickstart` |
| `platform/windows` | `WindowsServiceProcess` | Windows Service 레지스트리 엔트리 `my-supervisor-managed-<name>` | `sc create/start/stop/delete` |

**이름 네임스페이스**: 데몬용 unit (`my-supervisor.service`) 와 프로세스용 unit (`my-supervisor-managed-<name>.service`) 이 충돌하지 않도록 prefix 를 분리한다.

**유닛 잔재 청소**: `unregister` 가 실패하면 (권한·잠금 등) 잔재 unit 파일이 남는다. Production 단계에 `msv repair system-registrations` CLI 로 my-supervisor 네임스페이스 unit 을 스캔해 정리한다 (ROADMAP Phase 3).

### 11.3 로깅 연동

각 OS crate 가 `LogSink` port 의 보조 구현을 제공한다. Direct 모드는 stdout/stderr 파이프 캡처 (`infra/logging`) 를 쓰지만, SystemRegistered 모드는 OS 의 native 로그 저장소를 읽어야 한다.

- `platform/linux` · `JournaldLogSink`: Direct 는 `tracing-journald` 로 병행 출력, SystemRegistered 는 `journalctl --user -u my-supervisor-managed-<name>` 로 조회.
- `platform/macos` · `LaunchdLogSink`: launchd 가 stdout/stderr 를 `~/Library/Logs/` 로 리디렉트한 파일을 읽음. Unified logs 연동은 Post-Production.
- `platform/windows` · `EventLogSink`: Event Log 연동 (`eventlog` 크레이트).

## 12. Jobs (배치 스케줄러)

프로세스 감시와 별개로, **정해진 시각·주기·조건에 한 번씩 실행되는 배치 작업** 을 1급 개념으로 관리한다. PM2 는 이 영역이 약하고 OS crontab/Task Scheduler 는 플랫폼마다 문법·로그 수집이 다르므로, 데몬 자체에 내장 스케줄러를 둔다 (DD-022). Job 은 Process 와 다른 생명주기 (실행 → 정상 종료 기대) 를 가지므로 **별도 도메인 엔티티** 로 모델링한다 (DD-023). UI 상 `Jobs` 는 최상위 탭으로 노출.

### 12.1 개념

- **Job** — 사용자가 등록한 배치 정의 (command, args, env, trigger, 동시성 정책, 의존성 등)
- **JobRun** — Job 의 한 번 실행 인스턴스. trigger 발생마다 생성되며 상태는 `Pending → Running → Succeeded | Failed | Cancelled | Skipped` 로 전이
- **JobTrigger** — 4 종 one-of:
  - `Cron(expr)` — 5-field cron 표현식 (예: `0 */6 * * *`)
  - `Interval(duration)` — 주기 간격 (예: 5 분)
  - `OneShot(timestamp)` — 단발 예약
  - `DependsOn([job_id, …])` — **AND 시맨틱** (모두 성공해야 실행). 기본 on-success (DD-024)
- **OverlapPolicy** (`on_overlap`) — 이전 run 미완료 중 다음 trigger 시점 도달했을 때:
  - `Skip` *(기본)* — 이번 trigger 무시, `JobRun.state = Skipped` 로 기록
  - `Queue` — 순차 실행 (직전 run 종료 후 시작)
  - `Parallel` — 겹쳐서 병렬 실행 (리소스 폭주 위험이 있으므로 사용자가 명시적으로 선택)
- **DependencyFailurePolicy** (`on_dependency_failure`) — 의존하는 Job 중 하나라도 실패했을 때:
  - `Skip` *(기본)* — 트리거하지 않음, `JobRun.state = Skipped`
  - `RunAnyway` — 실패 무시하고 실행

### 12.2 Ports

- **`Scheduler`** — cron/interval/one-shot 평가 + 타이머 관리. 트리거 시점에 application 에 이벤트 발행
- **`JobRepository`** — Job 정의 + JobRun 이력 영속. **등록 시 순환 감지** 수행하고 감지 시 `cycle_detected`
- **`JobRunner`** — Job 을 transient 프로세스로 spawn. 내부적으로 `LifecycleController::spawn_detached` 재사용하고, stdout/stderr 는 `LogSink::for_job_run(run_id)` 로 수집. exit 대기 후 `JobRepository` 에 결과 확정

### 12.3 흐름

1. 사용자가 Job 등록 → `RegisterJob` use case → `JobRepository::save` (순환 감지 포함) → `Scheduler::register` (trigger 타입에 따라 cron 등록 / 타이머 등록 / 의존성 구독)
2. 트리거 발화 → `Scheduler` 이벤트 → `ScheduleTick` use case → `on_overlap` 에 따라 skip/queue/parallel 결정 → `JobRunner::run`
3. `JobRunner` 가 `JobRun` 생성 → `LifecycleController::spawn_detached` → 로그 수집 → exit 확보 → `JobRepository::save_finished`
4. `ObserveJobRunCompleted` use case 가 `DependsOn(…)` 에 이 Job 이 포함된 downstream Job 들을 순회하여 의존 조건 충족 여부 검사. 충족 시 `JobRunner::run` 호출

### 12.4 신뢰성 특성

- **`#[cfg(target_os)]` 없음**. cron 평가·타이머·spawn 전부 port 호출. Linux/macOS/Windows 동일 UI·동일 로그 파이프라인
- **순환 감지는 등록 시점**. 런타임 deadlock 을 원천 차단
- **Daemon 다운 시 스케줄링 미실행** — DD-022 의 트레이드오프. Production 단계에서 missed-runs 백필 기능 도입 예정 (ROADMAP Phase 3)
- **로그 수집 단위**: Job 전체가 아닌 **run 단위 파일** 로 저장. retention 은 `log_retention` 설정으로 run 수·기간 제한

### 12.5 API·CLI 요약

- REST: `/api/v1/jobs`, `/api/v1/jobs/{name}/trigger`, `/api/v1/jobs/{name}/runs`, `/api/v1/jobs/{name}/runs/{run_id}/logs` — 상세는 `API.md §2.3`
- WS 이벤트: `job.registered`, `job.run_scheduled`, `job.run_started`, `job.run_succeeded`, `job.run_failed`, `job.run_skipped`, `job.run_cancelled`
- CLI: `msv jobs ls / add / trigger / runs / logs / rm`

## 13. Rules (자동화)

프로세스·배치와 **동등한 제2 기둥**(DD-026)으로, **OS 이벤트에 반응해 액션을 실행** 한다. Hammerspoon 이 다루는 영역(파일·시스템 이벤트·핫키·윈도우)을 스크립팅 없이 GUI 로 제공한다. Job 이 **시간 기반**(예정된 시각·주기)인 데 반해 Rule 은 **이벤트 기반** 이며, 둘은 트리거 모델·이력 의미가 달라 별도 도메인 엔티티로 분리한다 (DD-023·DD-028). UI 상 `Rules` 는 자동화 기둥의 최상위 탭이다.

### 13.1 개념

- **Rule** — 사용자가 등록한 자동화 정의 (`trigger` + `actions[]` + `enabled`)
- **RuleFire** — Rule 의 한 번 발화 인스턴스. 이벤트 발생마다 생성되며 `Fired → ActionsSucceeded | ActionsFailed | Skipped` 로 전이. (Job 의 JobRun 과 대칭이되, 성패의 의미는 "액션 수행 결과")
- **RuleTrigger** — 3 종 one-of (모두 이벤트 기반):
  - `FileChange { path, kind }` — 파일/폴더 변경. **크로스플랫폼** (`EventSource`, `notify`)
  - `SystemEvent(kind)` — USB·WiFi·화면잠금·앱 실행 등. **macOS 전용 seam** (`SystemEventWatcher`)
  - `Hotkey(spec)` — 글로벌 핫키. **macOS 전용 seam** (`HotkeyBinder`)
- **RuleAction** — 순차 실행되는 액션 목록:
  - `StartProcess` / `StopProcess` / `TriggerJob` — **운영 기둥과 연계** (같은 코어의 use case·port 재사용)
  - `RunCommand` — 임의 명령 1회 실행
  - `Window(spec)` — 윈도우 이동·리사이즈·레이아웃. **macOS 전용 seam** (`WindowAutomation`)

### 13.2 Ports

| Port | 역할 | 종류 |
|---|---|---|
| `EventSource` | 파일/폴더 watch·타이머 이벤트 발행 | 크로스플랫폼 (`notify`) |
| `SystemEventWatcher` ✱ | macOS 시스템 이벤트 (NSWorkspace/IOKit) | 단일 구현 seam (macOS) |
| `HotkeyBinder` ✱ | 글로벌 핫키 등록·발화 (event tap) | 단일 구현 seam (macOS) |
| `WindowAutomation` ✱ | 윈도우 조작 (AX API) | 단일 구현 seam (macOS) |

✱ 표시는 §4.3 의 **단일 구현 seam** — trait 는 `core::ports` 에 있으나 구현자는 `platform/macos` 하나뿐, Linux/Windows stub 없음 (DD-027).

### 13.3 흐름

1. `EventSource`(또는 macOS 이벤트 seam)가 이벤트를 발행
2. `application::ObserveEvent` 가 수신 → `enabled = true` 이고 트리거가 매칭되는 Rule 선별
3. Rule 의 `actions[]` 를 순차 실행 — 프로세스/Job 액션은 기존 use case·port, 윈도우 액션은 `WindowAutomation` 으로 위임
4. 결과를 `RuleFire` 로 기록 (SQLite). 실패 액션은 `ActionsFailed` + 사유 저장

> 시간 기반 트리거는 이 경로가 아니라 `Scheduler`(§12)를 탄다. **`EventSource`(Rule) ↔ `Scheduler`(Job) 는 의도적으로 분리** 되어 있다 (DD-028).

### 13.4 macOS 전용 경계와 권한

- 윈도우·핫키·macOS 시스템 이벤트는 **GUI 세션 + TCC 권한** 이 필요하므로 `desktop`(Tauri, macOS) 호스트에서만 동작한다. 헤드리스 `daemon` 이나 비-macOS 호스트는 해당 seam 이 `None` 이라, 그런 트리거·액션을 포함한 Rule 은 **등록 시점에 `not supported on this platform` 으로 거부** 된다 (DD-027).
- 권한 모델 (DD-029): 윈도우 제어는 **Accessibility**, 글로벌 핫키는 **Input Monitoring** 권한이 필요하다. 미부여 시 해당 Rule 을 `enabled = false (권한 필요)` 로 표기하고 UI 에서 상태 + 시스템 설정 안내를 노출한다. **조용한 실패는 금지.**
- 파일 watch·타이머 트리거와 프로세스/Job/명령 액션은 권한과 무관하게 모든 호스트에서 동작한다.

### 13.5 API·CLI 요약

- `GET/POST/PATCH/DELETE /api/v1/rules` · `/api/v1/rules/{name}` — Rule CRUD
- `POST /api/v1/rules/{name}/fire` — 수동 시험 발화 (`TriggerRuleManual`)
- `GET /api/v1/rules/{name}/fires` — 발화 이력
- `GET /api/v1/automation/permissions` — macOS 권한 상태 (Accessibility / Input Monitoring)
- CLI: `msv rule ls` · `msv rule add -c rule.toml` · `msv rule run <name>` · `msv rule rm <name>`

상세 wire 스펙은 [API.md](./API.md).

---

## 14. 헬스체크

| 타입 | 동작 |
|---|---|
| `http` | 주기적 GET, 2xx 기대 |
| `tcp` | 포트 connect 성공 여부 |
| `exec` | 스크립트 실행 후 exit 0 기대 |

- `interval_sec`, `timeout_sec`, `failure_threshold`, `success_threshold` 설정
- 실패 임계 초과 시 프로세스 재시작 트리거 (옵션)
- 상태는 API를 통해 공개

## 15. 리소스 제한 [Post-Production]

Post-Production 단계에서 추가 (ROADMAP Phase 4 §1 참고). 최소한 다음을 지원:

- **메모리 한도 + OOM 재시작**
  - Linux: cgroup v2 memory.max
  - Windows: Job Object memory limit
  - macOS: 제한적 (RLIMIT_AS)
- CPU 쿼터는 복잡도 대비 수요가 낮아 후순위

## 16. 설정 파일 예시

```toml
[supervisor]
log_dir = "~/.local/share/my-supervisor/logs"
webui_port = 9876
webui_bind = "127.0.0.1"

[supervisor.autostart]
enabled = true
mode = "user"  # "user" | "system"

[[process]]
name = "api-server"
command = "node"
args = ["dist/server.js"]
cwd = "/home/user/projects/api"
env = { NODE_ENV = "production", PORT = "3000" }
management_mode = "direct"        # "direct" | { system_registered = { unit_name = "…" } }
lifecycle = "tied"                # Direct 모드에서만 유효
autostart = true

[process.restart]
policy = "on-failure"
max_retries = 10
backoff_initial_ms = 1000
backoff_max_ms = 60000
backoff_multiplier = 2.0
crash_loop_window_sec = 60
crash_loop_threshold = 5

[process.shutdown]
signal = "SIGTERM"
grace_period_sec = 10
force_signal = "SIGKILL"

[process.health_check]
type = "http"
endpoint = "http://localhost:3000/health"
interval_sec = 30
timeout_sec = 5
failure_threshold = 3

[process.logging]
format = "json"
max_file_size_mb = 100
max_files = 10
compress = true

[[process]]
name = "db"
command = "/usr/bin/postgres"
args = ["-D", "/var/lib/postgres"]
management_mode = { system_registered = { unit_name = "my-supervisor-managed-db" } }

# SystemRegistered 모드에서 restart 는 OS unit 파일의
# `Restart=on-failure` 같은 지시어로 변환되어 적용된다.
# 데몬 내부 RestartProcess use case 는 이 프로세스에 대해 no-op (§6.4).
[process.restart]
policy = "on-failure"
backoff_initial_ms = 5000
backoff_max_ms = 60000

[[job]]
name = "nightly-backup"
command = "/usr/local/bin/backup.sh"
args = ["--target", "/mnt/backup"]
cwd = "/var/app"
env = { BACKUP_KEY = "…" }
on_overlap = "skip"               # skip | queue | parallel (기본 skip)
on_dependency_failure = "skip"    # skip | run_anyway (기본 skip)
timeout_sec = 3600

[job.trigger]
type = "cron"                     # cron | interval | one_shot | depends_on
expr = "0 2 * * *"

[job.log_retention]
max_runs = 100                    # 최근 run 수 기준 보존
# 또는 max_age_days = 30

[[job]]
name = "post-backup-verify"
command = "/usr/local/bin/verify.sh"
on_overlap = "skip"

[job.trigger]
type = "depends_on"
jobs = ["nightly-backup"]         # AND 시맨틱, 기본 on-success
```

## 17. 보안

요구사항이 "단일 유저 / localhost / 인증 없음"이지만 다음 원칙을 적용:

- WebUI 바인딩을 `127.0.0.1`로 고정, 코드 레벨에서 `0.0.0.0` 불가
- Unix socket을 CLI↔데몬 통신에 옵션 제공 (파일 권한으로 접근 제어)
- 설정 파일 내 환경 변수에 시크릿이 포함될 수 있으므로 파일 권한 검사 (`0600` 권장)
- 로그 파일도 동일 권한 기본값
- 원격 접근이 필요해지는 시점에 인증 레이어 추가 (현 단계에선 구현 안 함)

## 18. 관찰성 (Observability)

- `/api/v1/metrics` — Prometheus 포맷 *(Post-Production)*
- 데몬 자체의 tracing 로그 (별도 파일)
- 프로세스별 재시작 히스토리 (SQLite)
- WebUI 대시보드에서 CPU/메모리 차트 *(Production 단계)*
