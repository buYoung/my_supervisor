# Architecture

본 문서는 `my-supervisor` 프로젝트의 전체 아키텍처, 컴포넌트 설계, 플랫폼별 구현 전략을 정리합니다.

## 1. 설계 원칙

1. **데몬과 UI의 분리** — UI 창을 닫거나 Tauri 앱이 종료되어도 관리 중인 프로세스는 영향을 받지 않는다.
2. **단일 통신 프로토콜** — Tauri WebView·브라우저·CLI가 모두 동일한 HTTP/WebSocket API를 사용한다.
3. **단일 코어 바이너리** — Desktop·Server 배포는 동일한 데몬 바이너리를 공유한다.
4. **GUI 우선, CLI 동등** — 주 사용자는 데스크톱 GUI, 그러나 CLI로도 동일 기능을 수행할 수 있어야 한다.
5. **Opt-in 시스템 통합** — 시스템 서비스 등록은 기본이 아니며 사용자가 명시적으로 활성화한다.
6. **Hexagonal 아키텍처 (Ports & Adapters)** — 도메인 로직은 OS·DB·네트워크 세부를 모른다. 모든 외부 의존은 `core` crate 의 port trait 로 추상화되고, `platform-*` · `infra-*` crate 가 adapter 로 구현한다. `#[cfg(target_os)]` 분기는 최상위 bin(`app/daemon` 등)에만 존재한다. 근거: DD-017 ~ DD-019.

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

단일 설치 프로그램이 세 개의 아티팩트를 설치:

- `msv-daemon` — 실제 supervisor 프로세스 (crate: `my-supervisor-app-daemon`)
- `msv` — CLI 클라이언트 (crate: `my-supervisor-app-cli`, PATH에 등록)
- Tauri 앱 — 데스크톱 GUI 셸 (crate: `my-supervisor-app-desktop`)

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

Hexagonal 레이어를 **Cargo workspace 의 개별 crate** 로 분리한다. 폴더는 카테고리 하위 중첩(`crates/platform/linux/`, `crates/infra/sqlite/`, `crates/app/daemon/`).

```
my-supervisor/
├── Cargo.toml                       # workspace (members 아래 참조)
├── Cargo.lock
├── README.md
├── docs/
├── .moon/                           # moon 워크스페이스 (apps/*, packages/*, crates/*)
├── apps/                            # (moon 규약 유지 — 현 단계 미사용)
├── packages/
│   └── ui/                          # React 프론트엔드 (Tauri WebView·외부 브라우저 공용)
│       ├── package.json
│       └── src/
│           ├── features/
│           │   ├── processes/       # 프로세스 목록·상세·제어
│           │   ├── logs/            # 로그 뷰어·follow
│           │   ├── daemon/          # 데몬 상태·리로드
│           │   └── settings/        # 설정 편집
│           ├── components/ui/       # shadcn 공용 컴포넌트
│           ├── services/            # HTTP/WS 클라이언트
│           └── shared/              # 타입·훅·유틸
├── crates/
│   ├── core/                        # 도메인 + port trait (std/serde/uuid 수준만 의존)
│   │   └── src/
│   │       ├── domain/              # Process, ProcessState, RestartPolicy, ChildHandle, …
│   │       └── ports/               # LifecycleController, ShutdownSignaler, AutoStartService,
│   │                                #  LogSink, StateRepository, HealthChecker, ConfigSource, …
│   ├── application/                 # use case (ports 에만 의존)
│   │   └── src/
│   │       ├── start_process.rs
│   │       ├── stop_process.rs
│   │       ├── restart_process.rs   # backoff + crash loop; OS 동작은 port 호출
│   │       ├── reconcile.rs
│   │       └── reload_config.rs
│   ├── shared/                      # wire types (HTTP/WS DTO, 설정 스키마 — serde)
│   │   └── src/
│   │       ├── api.rs               # REST 요청/응답
│   │       ├── events.rs            # WS 이벤트 페이로드
│   │       └── config.rs            # TOML 스키마
│   ├── config/                      # TOML 파싱·검증·watch (shared::config 재사용)
│   │   └── src/
│   │       ├── loader.rs
│   │       ├── validator.rs
│   │       └── watcher.rs           # notify 크레이트 — ConfigSource 구현
│   ├── infra/
│   │   ├── sqlite/                  # StateRepository 구현 (sqlx/SQLite)
│   │   ├── http/                    # axum HTTP + WS hub (core::ports 소비)
│   │   └── logging/                 # LogSink 구현 + 로테이션·백프레셔 (file-rotate, tracing)
│   ├── platform/
│   │   ├── linux/                   # prctl·PDEATHSIG·subreaper, systemd, journald
│   │   ├── macos/                   # kqueue shutdown hook, launchd plist, unified logs
│   │   └── windows/                 # Job Object, Service, Event Log, Task Scheduler, CTRL_BREAK
│   └── app/
│       ├── daemon/                  # bin (`msv-daemon`) — DI 조립
│       ├── cli/                     # bin (`msv`) — reqwest 클라이언트 (shared 타입 재사용)
│       └── desktop/                 # bin — Tauri shell (daemon spawn, tray, autostart, webview)
└── scripts/
    ├── install-server.sh
    └── build-packages.sh
```

**Workspace members 선언**:

```toml
# Cargo.toml (root)
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

Cargo 패키지명은 `my-supervisor-` prefix (예: `my-supervisor-core`, `my-supervisor-platform-linux`, `my-supervisor-app-daemon`). Rust `use` 경로는 자동으로 `my_supervisor_*` 로 변환된다. 바이너리 이름은 별개로 `msv` (CLI) / `msv-daemon` (데몬) 으로 설정한다.

### 3.1 의존성 방향 (단방향)

```
  app/*  ──▶  application  ──▶  core  ◀──  port trait 정의
    │              │                           ▲
    │              └────── shared ─────────────┤  (DTO 재사용)
    │                                          │
    ├──▶ infra/*    ────────────────────────── │  adapter 구현
    ├──▶ platform/* ────────────────────────── │  adapter 구현
    └──▶ config     ────────────────────────── ┘
```

규칙:

- `core` 는 다른 어떤 워크스페이스 crate 도 의존하지 않는다. 외부 crate 도 `serde`·`uuid`·`thiserror`·`tokio`(trait async) 등 의존성이 얇은 것만.
- `application` 은 `core` 만 의존. use case 는 port trait 를 호출할 뿐 adapter 타입을 직접 참조하지 않는다.
- `infra/*` · `platform/*` · `config` 는 `core`(+ 필요 시 `shared`) 만 의존. 서로 의존하지 않는다.
- `app/*` 이 조립 지점: `core` · `application` · 필요한 `infra/*` · `platform/*` · `config` · `shared` 를 모두 가져와 DI 로 조립한다. `#[cfg(target_os = "...")]` 분기는 여기서만 존재한다.

### 3.2 왜 이 구조인가

- **core / application 의 플랫폼-프리**: OS·DB·HTTP 세부가 도메인으로 누출되지 않아 Linux/macOS/Windows 가 각기 다르게 동작해도 use case 코드는 한 벌이다 (§6·§7·§11 참조). 테스트 시 `InMemoryStateRepository`, `NoopAutoStart` 같은 가짜 adapter 를 주입해 빠르게 검증.
- **OS별 adapter 의 물리적 격리**: `platform-linux` · `platform-macos` · `platform-windows` 가 각각 별도 crate 라 **빌드 타겟에 해당하는 crate 만 컴파일**된다. Linux 서버에서 `platform-macos` 소스 오류가 CI 에 새지 않고, `unsafe` · syscall 바인딩도 크레이트 경계 안에 갇힌다.
- **Infra 카테고리 분리**: SQLite → Postgres, axum → 다른 서버 프레임워크로의 교체가 `infra-sqlite` · `infra-http` 만 손대면 된다. `infra-logging` 도 같은 이유로 분리 — Phase 3 의 JSON 로테이션·백프레셔 변경이 다른 crate 를 건드리지 않는다.
- **Server 배포 크기 최소화**: `cargo build -p my-supervisor-app-daemon` 은 `app/desktop` (Tauri, GTK/WebView2) 을 건드리지 않는다. 플랫폼별로도 `platform/<현재_OS>` 만 링크된다.
- **`shared` 와 `core` 의 역할 분리**: `shared` 는 네트워크·파일로 나가는 **wire format** 만 (DTO, 설정 파일 스키마). `core` 는 **도메인 모델** 과 **port trait**. 둘을 섞지 않아야 API 스펙이 도메인 변경에 휘둘리지 않고, 역으로 도메인이 wire 포맷 호환성에 묶이지 않는다.
- **Feature 단위 프론트엔드**: `packages/ui` 의 `features/*` 가 백엔드 port 분리와 대칭. 각 feature 폴더는 자기 UI + service 호출 + 훅을 자기 안에 둔다. 공용 shadcn 컴포넌트는 `components/ui`, HTTP/WS 클라이언트는 `services/` 에 분리.

## 4. 컴포넌트 설계

컴포넌트는 **(a) 실행 형태** (bin/lib) 와 **(b) 레이어 위치** 를 교차해서 본다. §3 의 5-레이어(core / application / shared+config / infra / platform / app) 를 기준으로 정리한다.

### 4.1 App — 바이너리 조립 지점

실제로 빌드되는 최종 산출물. Hexagonal 의 "composition root" 역할만 한다. 도메인 로직은 들어가지 않는다.

#### 4.1.1 `app/daemon` (bin `msv-daemon`)

실제 supervisor. 혼자서 완결적으로 동작하며, UI 없이도 CLI와 WebUI로 관리 가능.

**책임:**
- `core` 의 도메인 타입과 port trait 를 임포트
- 현재 타겟 OS 에 맞는 `platform/*` adapter 를 `#[cfg(target_os = "...")]` 로 선택 → trait object 로 `application` 에 주입
- `infra/sqlite`, `infra/http`, `infra/logging`, `config` 를 조립
- 런타임 진입점: tokio runtime 구성, 시그널 처리, graceful shutdown

**책임 경계:** 이 crate 에는 `if` 분기 이상의 로직이 들어가지 않아야 한다. 테스트는 `application` 에서 한다.

**주요 크레이트:**

| 역할 | 크레이트 |
|---|---|
| 비동기 런타임 | `tokio` |
| 시그널 | `signal-hook`, `signal-hook-tokio` |
| DI / 에러 | `anyhow`, `thiserror` |

`sqlx`, `axum`, `nix`, `windows` 등은 각 infra/platform crate 의 내부 의존이며 `app/daemon` 은 직접 알 필요 없다.

#### 4.1.2 `app/cli` (bin `msv`)

데몬의 HTTP API를 호출하는 얇은 클라이언트. `shared` crate 의 wire 타입을 재사용하므로 API 변경이 컴파일 타임에 잡힌다.

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

#### 4.1.3 `app/desktop` (Tauri bin)

데몬의 프론트엔드 셸. 로직은 최소화하고 대부분 Rust 단에서는 "데몬 관리"만 담당.

**책임:**
- 앱 시작 시 데몬 헬스체크 → 없으면 `msv-daemon` spawn
- WebView 에 `http://127.0.0.1:<port>` 로드 (번들된 `packages/ui` 를 서빙)
- 트레이 아이콘 + 메뉴
- 네이티브 알림 (crash loop 진입 등)
- OS 자동 시작 등록 (user-level) — `platform/*` 의 `AutoStartService` adapter 재사용 가능

**Tauri 플러그인:**
- `tauri-plugin-autostart` — 로그인 시 자동 시작 (간이 경로; 정식 OS 서비스 등록은 `platform/*` 사용)
- `tauri-plugin-single-instance` — 이중 실행 방지
- `tauri-plugin-notification` — 네이티브 알림
- `tauri-plugin-shell` — 브라우저 열기 등

**프론트엔드:** `packages/ui` 빌드 산출물을 Tauri WebView 와 외부 브라우저가 **동일하게** 로드. Tauri 의 `invoke` IPC 에 의존하지 않고 순수 HTTP/WebSocket 만 사용 → 같은 UI 가 Server 배포의 브라우저 접속에도 그대로 작동. 근거는 DD-016.

### 4.2 Application — Use Case 레이어

`crates/application`. `core::ports` 의 trait 만 소비하는 순수 로직. 각 use case 는 한 파일·한 struct.

**대표 use case (PoC·MVP·Production 순 확장):**

- `StartProcess` — spec 조회 → `LifecycleController::spawn_*` → `StateRepository::save_started`
- `StopProcess` — 상태 검증 → `ShutdownSignaler::request_graceful` → `StateRepository::save_stopped`
- `RestartProcess` — backoff 계산 + crash loop 감지 후 `Stop` → `Start` 체이닝 (OS 차이 없음)
- `ReconcileChildren` — 10초 주기. `LifecycleController::probe_alive` + 신원 3종 세트 (§10) 검증 → 사망/고아 판정
- `ReloadConfig` — `ConfigSource::load` → diff → 영향 받는 프로세스만 재시작
- `RegisterAutoStart` / `UnregisterAutoStart` — `AutoStartService` 호출

Use case 는 OS 를 모른다 (#[cfg] 없음). 모든 플랫폼 차이는 port 호출로 위임한다.

### 4.3 Core — 도메인 + Ports

`crates/core`. 프로젝트의 중심. 다른 워크스페이스 crate 에 **의존하지 않는다**.

**`core::domain`** — 값 객체와 엔티티.

```rust
// crates/core/src/domain/process.rs 발췌
pub struct ProcessSpec {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub lifecycle: LifecycleMode,       // Tied | Detached
    pub restart: RestartPolicy,
    pub shutdown: ShutdownPolicy,
    /* … */
}

pub enum ProcessState { Starting, Running, Stopping, Crashed, Stopped }

pub struct ChildHandle {
    pub process_id: Uuid,       // MYSUPERVISOR_PROCESS_ID
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    /* opaque OS 핸들은 platform crate 가 소유 */
}
```

**`core::ports`** — trait 목록.

| Port trait | 역할 | adapter 위치 |
|---|---|---|
| `LifecycleController` | tied/detached spawn, process group 제어, alive 확인 | `platform/*` |
| `ShutdownSignaler` | graceful → force kill 시퀀스 | `platform/*` |
| `AutoStartService` | OS 자동 시작 등록/해제 | `platform/*` |
| `StateRepository` | 프로세스·히스토리·메트릭 영속화 | `infra/sqlite` (+ 테스트용 in-memory) |
| `LogSink` | 자식 stdout/stderr 스트림 수집·로테이션 | `infra/logging` |
| `HttpServer` | HTTP/WS 엔드포인트 호스팅 | `infra/http` |
| `ConfigSource` | TOML 로드·검증·watch | `config` |
| `HealthChecker` | HTTP / TCP / exec 헬스체크 | `infra/*` (Phase 3) |
| `SystemClock` | 시간 의존 (테스트 가짜 주입용) | `infra/*` |

### 4.4 Shared — Wire Types

`crates/shared`. HTTP/WS DTO 와 TOML 설정 스키마만 담는다. `core` 와 분리된 이유: wire format 호환성 제약(`#[serde(rename_all = "snake_case")]` 같은 어노테이션)이 도메인 타입으로 누출되지 않게 하기 위함.

```rust
// crates/shared/src/api.rs 예시
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

`infra/http` 와 `app/cli` 가 같은 DTO 를 임포트하기 때문에 API 변경이 양쪽에 동시 반영된다 (DD-003 의 "단일 통신 프로토콜" 을 타입 레벨에서 강제).

### 4.5 Config

`crates/config`. `ConfigSource` port 의 참조 구현. `shared::config` 스키마를 사용하여 TOML 을 파싱하고, `notify` 크레이트로 파일 변경을 감지한다. `core` 외에는 의존하지 않기 때문에 테스트에서 `InMemoryConfigSource` 와 교체 가능.

### 4.6 Infrastructure Adapters

`crates/infra/*`. 기술 스택에 종속적인 port 구현.

| Crate | 구현 port | 주요 의존 |
|---|---|---|
| `infra/sqlite` | `StateRepository` | `sqlx` (SQLite, WAL 모드) |
| `infra/http` | `HttpServer` (+ WS hub) | `axum`, `tower`, `tokio-tungstenite`, `my-supervisor-shared` |
| `infra/logging` | `LogSink` + 로테이션 | `tracing`, `tracing-subscriber`, `file-rotate` |

교체 가능성: SQLite → Postgres 시 `infra/postgres` 를 새로 만들고 `app/daemon` 의 DI 만 교체하면 된다. `core` · `application` 은 무변경.

### 4.7 Platform Adapters

`crates/platform/*`. OS 에 종속적인 port 구현. 각 crate 는 **자기 OS 에서만 컴파일**되도록 `[target]` 블록 또는 `#[cfg]` 로 가드한다.

| Crate | 대표 구현 | OS-native API |
|---|---|---|
| `platform/linux` | `LinuxLifecycle`, `UnixShutdown`, `SystemdUserUnit`, `JournaldLogSink`(선택) | `prctl`(`nix`), `signal-hook`, `systemd` + `tracing-journald` |
| `platform/macos` | `MacLifecycle`, `UnixShutdown`, `LaunchdAgent` | `kqueue`, `launchctl`, `plist` |
| `platform/windows` | `WindowsLifecycle`, `WindowsShutdown`, `TaskSchedulerEntry`/`WindowsService`, `EventLogSink`(선택) | Job Object, `CTRL_BREAK_EVENT`, `sc`/Task Scheduler, Event Log (`windows` crate) |

세 crate 가 같은 port trait (`LifecycleController` 등) 을 구현하기 때문에 `app/daemon` 에서는:

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
│   ├── processes/    # 목록 / 상세 / 시작·중지·재시작 버튼 / 설정 폼
│   ├── logs/         # 로그 뷰어 + follow
│   ├── daemon/       # 데몬 상태·리로드·종료
│   └── settings/     # 전역 설정 편집
├── components/ui/    # shadcn 공용 컴포넌트
├── services/         # api.ts (REST), events.ts (WS)
└── shared/           # 훅·유틸·타입 (shared crate 의 API 타입을 TS 로 매핑)
```

각 feature 폴더는 자기 UI 컴포넌트, 자기 service 호출, 자기 훅을 가진다. 크로스 feature 공유가 필요하면 `shared/` 로 승격한다. Tauri 의 `invoke` IPC 에는 의존하지 않으므로 번들은 Tauri 내부·외부 브라우저에서 동일하게 작동.

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
// crates/core/src/ports/lifecycle.rs
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

`application::StartProcess` 는 위 표의 어느 항목도 직접 알지 않는다:

```rust
// crates/application/src/start_process.rs 발췌
pub async fn start_process(
    spec: &ProcessSpec,
    lifecycle: &dyn LifecycleController,
    repo: &dyn StateRepository,
) -> Result<ChildHandle, StartError> {
    let handle = match spec.lifecycle {
        LifecycleMode::Tied     => lifecycle.spawn_tied(spec).await?,
        LifecycleMode::Detached => lifecycle.spawn_detached(spec).await?,
    };
    repo.save_started(&handle).await?;
    Ok(handle)
}
```

### 6.3 시작 시퀀스

1. 설정에서 프로세스 정의 로드
2. 작업 디렉터리 확인
3. 환경 변수 구성 (시스템 환경 + 사용자 정의 + `MYSUPERVISOR_PROCESS_ID=<uuid>` 태그)
4. Unix: `pre_exec`에서 `prctl` 설정 → `setsid`/`setpgid`
5. Windows: Job Object 생성 → 프로세스 spawn (suspended) → Job에 할당 → resume
6. stdout/stderr 파이프를 비동기 reader로 연결
7. SQLite에 시작 기록 (pid, start_time, uuid, command hash)

## 7. 시그널 처리

### 7.1 Port: `ShutdownSignaler`

자식 프로세스로 "정지하라" 는 신호를 보내는 방식은 OS 마다 다르지만, `application` 레이어에는 "정상 요청 → grace period → 강제 종료" 라는 동일한 논리가 있다. 이를 port 로 추상화한다.

```rust
// crates/core/src/ports/shutdown.rs
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

## 11. 시스템 서비스 통합 (Opt-in)

사용자가 UI 또는 CLI에서 토글로 활성화/비활성화. OS 별 파일 포맷·명령이 완전히 다르기 때문에 `AutoStartService` port 로 묶고 `platform/*` crate 가 구현한다 (DD-018).

### 11.1 두 가지 모드

| 모드 | 데몬이 관리 | 시스템이 관리 |
|---|---|---|
| **Process-managed** *(기본)* | 데몬이 프로세스를 supervisor로 직접 관리 | 시스템 서비스 등록 안 함 |
| **System-integrated** *(opt-in)* | 데몬 자체를 시스템 서비스로 등록 | 시스템이 데몬을 관리, 데몬이 자식을 관리 |

사용자는 **프로세스별로도** System-integrated 모드 선택 가능 (개별 프로세스를 systemd unit 등으로 등록하여 데몬 밖에서도 살아있게).

### 11.2 Port: `AutoStartService`

```rust
// crates/core/src/ports/autostart.rs
#[async_trait]
pub trait AutoStartService: Send + Sync {
    async fn enable(&self, scope: AutoStartScope) -> Result<(), AutoStartError>;
    async fn disable(&self) -> Result<(), AutoStartError>;
    async fn status(&self) -> Result<AutoStartStatus, AutoStartError>;
}

pub enum AutoStartScope { User, System } // user-level vs 시스템 전역
```

### 11.3 OS별 adapter

| OS Crate | 구현체 | 파일 경로 | 제어 |
|---|---|---|---|
| `platform/linux` | `SystemdUserUnit` | `~/.config/systemd/user/my-supervisor.service` | `systemctl --user enable/start my-supervisor` |
| `platform/macos` | `LaunchdAgent` | `~/Library/LaunchAgents/com.my-supervisor.daemon.plist` | `launchctl bootstrap gui/$(id -u) ...` |
| `platform/windows` | `TaskSchedulerEntry` 또는 `WindowsService` | Task Scheduler XML · `sc create my-supervisor` | `sc start/stop my-supervisor` |

UI/CLI는 `AutoStartService` trait 만 호출 → OS 분기는 `app/daemon` 의 DI 지점에만 존재.

### 11.4 로깅 연동

각 OS crate 가 `LogSink` port 의 보조 구현을 선택적으로 제공할 수 있다.

- `platform/linux` · `JournaldLogSink`: `tracing-journald`로 journald 병행 출력. `journalctl --user -u my-supervisor`로 조회.
- `platform/macos`: launchd 가 stdout/stderr 를 `~/Library/Logs/` 로 리디렉트. Unified logs 연동은 Post-Production.
- `platform/windows` · `EventLogSink`: Event Log 연동 (`eventlog` 크레이트).

## 12. 헬스체크

| 타입 | 동작 |
|---|---|
| `http` | 주기적 GET, 2xx 기대 |
| `tcp` | 포트 connect 성공 여부 |
| `exec` | 스크립트 실행 후 exit 0 기대 |

- `interval_sec`, `timeout_sec`, `failure_threshold`, `success_threshold` 설정
- 실패 임계 초과 시 프로세스 재시작 트리거 (옵션)
- 상태는 API를 통해 공개

## 13. 리소스 제한 [Post-Production]

Post-Production 단계에서 추가 (ROADMAP Phase 4 §1 참고). 최소한 다음을 지원:

- **메모리 한도 + OOM 재시작**
  - Linux: cgroup v2 memory.max
  - Windows: Job Object memory limit
  - macOS: 제한적 (RLIMIT_AS)
- CPU 쿼터는 복잡도 대비 수요가 낮아 후순위

## 14. 설정 파일 예시

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
lifecycle = "tied"
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
```

## 15. 보안

요구사항이 "단일 유저 / localhost / 인증 없음"이지만 다음 원칙을 적용:

- WebUI 바인딩을 `127.0.0.1`로 고정, 코드 레벨에서 `0.0.0.0` 불가
- Unix socket을 CLI↔데몬 통신에 옵션 제공 (파일 권한으로 접근 제어)
- 설정 파일 내 환경 변수에 시크릿이 포함될 수 있으므로 파일 권한 검사 (`0600` 권장)
- 로그 파일도 동일 권한 기본값
- 원격 접근이 필요해지는 시점에 인증 레이어 추가 (현 단계에선 구현 안 함)

## 16. 관찰성 (Observability)

- `/api/v1/metrics` — Prometheus 포맷 *(Post-Production)*
- 데몬 자체의 tracing 로그 (별도 파일)
- 프로세스별 재시작 히스토리 (SQLite)
- WebUI 대시보드에서 CPU/메모리 차트 *(Production 단계)*
