# my-supervisor

> macOS 로컬 프로세스·배치 운영 콘솔.

> **현재 지원 범위:** macOS 로컬 전용입니다. `Process`·`Job`·로그·CLI·Desktop의 구현된 운영 기능만 지원합니다. Linux/Windows, Rules 자동화, 원격 운영은 설계 목표이며 현재 지원하지 않습니다.

## 개요

`my-supervisor` (CLI 명령어 `msv`) 는 개인 개발자의 macOS 로컬 환경에서 프로세스와 배치 Job을 관리하는 도구입니다.

- **운영 (Operations)** — PM2 처럼 장시간 실행 프로세스를 감시·재시작하고, cron 처럼 예약·주기·의존성 기반 배치 Job 을 관리합니다.
- **자동화 (Automation)** — Rules 기반 이벤트 자동화는 목표 설계에 포함되지만 현재 구현·지원하지 않습니다.

현재 도메인 지원 범위는 **운영 = {`Process`, `Job`}** 입니다. `Rule`은 아직 공개 계약에 포함되지 않습니다.

구현된 기능은 Desktop과 CLI에서 제공하며, HTTP API는 인증 없는 loopback 전용입니다.

### 플랫폼 범위 (한눈에)

| 기둥 | 기능 | 플랫폼 |
|---|---|---|
| 운영 | 프로세스 관리, 배치 Job | **macOS 지원** |
| 자동화 | Rules와 이벤트 트리거 | **미지원·설계 초안** |
| 다른 OS | Windows / Linux | **미지원** |

> 윈도우 관리·글로벌 핫키는 OS 마다 모델이 근본적으로 다르기 때문에(macOS Accessibility · X11/Wayland · Windows 가 서로 다른 패러다임) 여러 OS 구현을 강제하는 크로스 플랫폼 추상화를 만들지 않고, 구현자가 `platform/macos` 하나뿐인 **단일 구현 seam**(빈 stub 없음)으로 둡니다. 근거: [DESIGN_DECISIONS.md](./DESIGN_DECISIONS.md) DD-027.

### 현재 핵심 가치

- **macOS 로컬 운영**: 지원 범위를 한정해 프로세스·배치 실행과 복구 동작을 우선 완성합니다.
- **GUI 우선**: Tauri 기반 네이티브 데스크톱 앱에서 구현된 상태·프로세스·Job·로그 기능을 제공합니다.
- **Process · Job 이원 모델**: 장시간 실행 프로세스(`Process`)와 cron/interval/one-shot/의존성 배치(`Job`)를 구분해 관리합니다.
- **프로세스 관리 모드 이원화**: 각 Process 를 **Direct** (데몬이 직접 관리) 또는 **SystemRegistered** (OS 서비스 매니저에 등록) 중 선택. 런타임에 양방향 전환 가능.
- **테마 동등 지원**: 다크/라이트 모드를 1 급 지원 (shadcn/ui CSS 변수 기반).
- **지원 여부의 명시성**: 연결되지 않은 기능은 성공하는 것처럼 표시하지 않고 미지원 상태로 구분합니다.
- **시스템 통합**: macOS `launchd`를 통한 `SystemRegistered` 프로세스 관리를 제공합니다.

## 아키텍처 한눈에

```
   데스크톱 (macOS)                      서버 (헤드리스)
  ┌─────────────────────────┐         ┌─────────────────────────┐
  │ desktop (Tauri)        │         │ msv-daemon launcher     │
  │  · WebView · tray       │         │  · GUI 없음             │
  │  · 자동화 권한 (AX/핫키) │         │  · 운영 전용            │
  │ ─────────────────────── │         │ ─────────────────────── │
  │ daemon runtime 재사용│         │ daemon runtime 실행 │
  │ + invoke/devBridge       │         │ + axum HTTP/WS 내장      │
  └───────────┬─────────────┘         └───────────┬─────────────┘
              │ HTTP/WS (localhost)               │ HTTP/WS
        ┌─────┴──────┐                      ┌─────┴──────┐
        │ 브라우저·msv │                      │ 브라우저·msv │
        └────────────┘                      └────────────┘

  현재 core가 관리하는 대상:
   • Process(자식 프로세스) · Job(배치)        ─ macOS 로컬 운영
   • Rule                                        ─ 미지원·설계 초안
```

`core`/`application` 은 도메인·유스케이스 라이브러리이고, **`daemon`은 이를 실제 adapter와 조립하는 공통 런타임** 입니다. 데스크톱(`desktop`)은 이 런타임을 인프로세스로 재사용하고, macOS headless 실행은 같은 런타임을 `msv-daemon` launcher로 실행합니다. 데스크톱에서 창을 닫아도 Tauri 가 tray 로 상주하여 관리 중인 프로세스가 유지됩니다 — **별도 데몬 프로세스를 spawn 하지 않습니다.**

내부 모듈은 Hexagonal (Ports & Adapters) 로 분리되어 있습니다. 도메인 로직은 OS·DB·HTTP 세부를 모르고, 플랫폼·인프라 adapter crate 가 port trait 를 구현합니다. **단, 윈도우·핫키처럼 OS 마다 개념 자체가 다른 자동화 기능은 여러 OS 구현을 강제하지 않고, 구현자가 `platform/macos` 하나뿐인 단일 구현 seam 으로 둡니다** (DD-027). `application` 은 `Option<&dyn …>` 로만 소비하므로 `#[cfg]`·`platform/*` 의존이 침투하지 않습니다.

```
  cli ─┐
           ├──▶ daemon runtime ──▶ application ──▶ core
  desktop ┘              │              │             ▲
                             │              └── shared ───┤
                             ├──▶ infra/*    ──────────── │
                             ├──▶ platform/* ──────────── │
                             │        └ macos: 단일 구현 seam
                             └──▶ config     ──────────── ┘
```

상세는 [ARCHITECTURE.md §3](./ARCHITECTURE.md#3-모노레포-구조) · [DESIGN_DECISIONS.md DD-017~DD-020, DD-026~DD-029](./DESIGN_DECISIONS.md) 참조.

## 배포 형태

| 배포 | 타깃 | 구성 | 자동화 |
|---|---|---|---|
| Desktop (macOS) | 개인 개발자, 로컬 작업 | Tauri 앱(코어 임베드) + CLI | Rules 미지원 |
| Headless (macOS) | 로컬 작업 | `msv-daemon` + CLI | Rules 미지원 |
| Windows / Linux / 원격 서버 | 향후 범위 | 현재 배포 불가 | 미지원 |

## 기술 스택

- **언어**: Rust (전 컴포넌트)
- **UI 셸**: Tauri v2
- **데몬**: tokio + axum
- **프론트엔드**: React + Vite + Tailwind + shadcn/ui (`crates/desktop/ui`, feature 단위 모듈 — DD-020). 다크/라이트 양립.
- **현재 상위 IA**:
  - **운영**: Processes · Jobs · Logs
  - **공통**: Daemon · Settings
- **영속화**: TOML (설정) + SQLite (런타임 상태 · JobRun 이력)
- **배치 스케줄러**: 데몬 내장 (`infra/scheduler`, `tokio-cron-scheduler` 기반). cron 5-field + interval + one-shot + 의존성.
- **모듈 구조**: Hexagonal Cargo workspace — `core` / `application` / `shared`·`config` / `infra/*` / `platform/*` / `daemon`·`cli`·`desktop`. 데스크톱 UI는 `desktop/ui`에 포함한다. Cargo 패키지 prefix `my-supervisor-`, 바이너리는 `msv` · `msv-daemon`.

## 문서

- [아키텍처](./ARCHITECTURE.md) — 컴포넌트 구조, 기술 결정, OS별 구현 세부
- [로드맵](./ROADMAP.md) — 단계별 계획 (walking skeleton → 슬라이스 확장)
- [설계 결정 기록](./DESIGN_DECISIONS.md) — 주요 선택의 근거와 대안
- [개발 가이드](./DEVELOPMENT.md) — 로컬 환경 세팅·빌드·테스트
- [CLI·앱 로컬 빌드 가이드](./BUILD.md) — CLI 실행, 앱 개발 모드, 로컬 `.app` 생성
- [API 레퍼런스](./API.md) — HTTP/WebSocket 스펙

## 현재 상태

현재 구현 범위는 **macOS 로컬 운영 MVP**입니다. 프로세스·Job·로그·설정 복구의 기반과 CLI/Desktop 일부가 구현되어 있습니다. Rules 자동화, Linux/Windows 런타임, 원격 운영은 지원하지 않습니다. 설계 문서의 향후 구조와 현재 지원 기능을 구분해 해석해야 합니다.

## 라이선스

*TBD*
</content>
</invoke>
