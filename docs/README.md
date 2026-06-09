# my-supervisor

> 크로스 플랫폼 운영 + macOS 자동화 콘솔. **프로세스·배치 운영**과 **이벤트 기반 자동화**를 한 GUI 에서.

## 개요

`my-supervisor` (CLI 명령어 `msv`) 는 개인 개발자와 소규모 운영 환경을 위한 도구로, **두 기둥을 동등하게** 지원합니다.

- **운영 (Operations)** — PM2 처럼 장시간 실행 프로세스를 감시·재시작하고, cron 처럼 예약·주기·의존성 기반 배치 Job 을 관리합니다.
- **자동화 (Automation)** — Hammerspoon 처럼 OS 이벤트(파일·폴더 변경, 시스템 이벤트, 글로벌 핫키)에 반응해 액션을 실행하고, macOS 윈도우를 제어합니다. **단, 복잡한 스크립팅 없이 GUI 로.**

도메인으로는 **운영 = {`Process`, `Job`}**, **자동화 = {`Rule`}** 로 매핑됩니다.

두 기둥 모두 **GUI 우선**으로 설계되어 설정과 운영이 쉽고, 서버 환경에서는 운영 기능을 **CLI 와 WebUI** 로 동일하게 제공합니다.

### 플랫폼 범위 (한눈에)

| 기둥 | 기능 | 플랫폼 |
|---|---|---|
| 운영 | 프로세스 관리, 배치 Job | **크로스 플랫폼** (Windows / macOS / Linux) |
| 자동화 | 이벤트 트리거 (파일·폴더 watch, 타이머) | **크로스 플랫폼** |
| 자동화 | 윈도우 관리, 글로벌 핫키, 깊은 시스템 이벤트 | **macOS 전용** |

> 윈도우 관리·글로벌 핫키는 OS 마다 모델이 근본적으로 다르기 때문에(macOS Accessibility · X11/Wayland · Windows 가 서로 다른 패러다임) 여러 OS 구현을 강제하는 크로스 플랫폼 추상화를 만들지 않고, 구현자가 `platform/macos` 하나뿐인 **단일 구현 seam**(빈 stub 없음)으로 둡니다. 근거: [DESIGN_DECISIONS.md](./DESIGN_DECISIONS.md) DD-027.

### 핵심 가치

- **운영 + 자동화 동등 이원**: 어느 한쪽이 부가 기능이 아닙니다. 프로세스·배치 운영과 이벤트 기반 자동화를 같은 콘솔에서 1 급으로 다룹니다.
- **GUI 우선**: Tauri 기반 네이티브 데스크톱 앱. 설정·모니터링·로그 확인이 전부 클릭 단위.
- **Process · Job · Rule 삼원 모델**: 장시간 실행 프로세스(`Process`), cron/interval/one-shot/의존성 배치(`Job`), 이벤트→액션 자동화(`Rule`) 를 **하나의 GUI** 에서 관리. Job 스케줄링과 Rule 이벤트 처리는 OS 도구에 위임하지 않고 데몬이 자체 실행.
- **이벤트 기반 자동화 (macOS)**: 파일 변경·시스템 이벤트·핫키를 트리거로, 프로세스 제어·Job 실행·명령 실행·윈도우 배치를 액션으로. Hammerspoon 의 강력함을 스크립팅 없이.
- **프로세스 관리 모드 이원화**: 각 Process 를 **Direct** (데몬이 직접 관리) 또는 **SystemRegistered** (OS 서비스 매니저에 등록) 중 선택. 런타임에 양방향 전환 가능.
- **테마 동등 지원**: 다크/라이트 모드를 1 급 지원 (shadcn/ui CSS 변수 기반).
- **자동화는 "견고하거나 없거나"**: 오작동하는 핫키나 사용자와 싸우는 윈도우 매니저는 일상 사용에 파괴적입니다. 자동화 기능은 어설프게 얕게 내지 않고, 견고하게 내거나 아예 내지 않습니다.
- **서버 친화**: 헤드리스 환경에서도 운영 기능을 CLI + WebUI 로 동일하게 제어 (자동화의 GUI 의존 기능 제외).
- **시스템 통합**: systemd / launchd / Windows Service 와 opt-in 연동 (데몬 자체의 자동 시작).

## 아키텍처 한눈에

```
   데스크톱 (macOS)                      서버 (헤드리스)
  ┌─────────────────────────┐         ┌─────────────────────────┐
  │ app/desktop (Tauri)     │         │ app/daemon              │
  │  · WebView · tray       │         │  · GUI 없음             │
  │  · 자동화 권한 (AX/핫키) │         │  · 운영 전용            │
  │ ─────────────────────── │         │ ─────────────────────── │
  │ core + application 임베드│         │ core + application 임베드│
  │ + axum HTTP/WS 내장      │         │ + axum HTTP/WS 내장      │
  └───────────┬─────────────┘         └───────────┬─────────────┘
              │ HTTP/WS (localhost)               │ HTTP/WS
        ┌─────┴──────┐                      ┌─────┴──────┐
        │ 브라우저·msv │                      │ 브라우저·msv │
        └────────────┘                      └────────────┘

  core 가 관리하는 대상:
   • Process(자식 프로세스) · Job(배치)        ─ 운영 (크로스 플랫폼)
   • Rule = OS 이벤트 → 액션 (윈도우·핫키)      ─ 자동화 (윈도우/핫키는 데스크톱 macOS 전용)
```

`core`/`application` 은 공유 라이브러리이고, **데스크톱은 Tauri 앱(`app/desktop`)이, 서버는 헤드리스 데몬(`app/daemon`)이 각각 이 코어를 임베드** 합니다(DD-002). 두 호스트 모두 axum HTTP/WS 서버를 내장하므로 CLI·외부 브라우저가 동일 API 로 붙습니다. 데스크톱에서 창을 닫아도 Tauri 가 tray 로 상주하여 자식 프로세스·등록된 Rule 이 유지됩니다 — **별도 데몬 프로세스를 spawn 하지 않습니다.**

내부 모듈은 Hexagonal (Ports & Adapters) 로 분리되어 있습니다. 도메인 로직은 OS·DB·HTTP 세부를 모르고, 플랫폼·인프라 adapter crate 가 port trait 를 구현합니다. **단, 윈도우·핫키처럼 OS 마다 개념 자체가 다른 자동화 기능은 여러 OS 구현을 강제하지 않고, 구현자가 `platform/macos` 하나뿐인 단일 구현 seam 으로 둡니다** (DD-027). `application` 은 `Option<&dyn …>` 로만 소비하므로 `#[cfg]`·`platform/*` 의존이 침투하지 않습니다.

```
  app/*  ──▶ application ──▶ core  ◀── port trait 정의 (크로스 플랫폼)
    │             │             ▲
    │             └── shared ───┤  (DTO 재사용)
    │                           │
    ├──▶ infra/*    ─────────── │   adapter 구현
    ├──▶ platform/* ─────────── │   adapter 구현
    │        └ macos: 윈도우·핫키 단일 구현 (구현자 1개·stub 없음)
    └──▶ config     ─────────── ┘
```

상세는 [ARCHITECTURE.md §3](./ARCHITECTURE.md#3-모노레포-구조) · [DESIGN_DECISIONS.md DD-017~DD-020, DD-026~DD-029](./DESIGN_DECISIONS.md) 참조.

## 배포 형태

| 배포 | 타깃 | 구성 | 자동화 |
|---|---|---|---|
| Desktop (macOS) | 개인 개발자, 로컬 작업 | Tauri 앱(코어 임베드) + CLI | 윈도우·핫키 포함 전체 |
| Desktop (Win/Linux) | 개인 개발자, 로컬 작업 | Tauri 앱(코어 임베드) + CLI | 크로스 플랫폼 트리거만 |
| Server | 서버, WSL, CI, 컨테이너 | 헤드리스 데몬(코어 임베드) + CLI | 크로스 플랫폼 트리거만 |

## 기술 스택

- **언어**: Rust (전 컴포넌트)
- **UI 셸**: Tauri v2
- **데몬**: tokio + axum
- **프론트엔드**: React + Vite + Tailwind + shadcn/ui (`packages/ui`, feature 단위 모듈 — DD-020). 다크/라이트 양립.
- **상위 IA (동등 이원)**:
  - **운영**: Processes · Jobs · Logs
  - **자동화**: Rules (이벤트→액션, 윈도우·핫키 포함)
  - **공통**: Daemon · Settings
- **영속화**: TOML (설정) + SQLite (런타임 상태 · JobRun 이력 · Rule 실행 이력)
- **배치 스케줄러**: 데몬 내장 (`infra/scheduler`, `tokio-cron-scheduler` 기반). cron 5-field + interval + one-shot + 의존성.
- **자동화 엔진**: 데몬 내장. 이벤트 소스는 크로스 플랫폼(`notify` 파일 watch, 타이머)과 macOS 전용(Accessibility API 윈도우 제어, event tap 글로벌 핫키, NSWorkspace/IOKit 시스템 이벤트)으로 구성.
- **모듈 구조**: Hexagonal Cargo workspace — `core` / `application` / `shared`·`config` / `infra/*` / `platform/*` / `app/*`. Cargo 패키지 prefix `my-supervisor-`, 바이너리는 `msv` · `msv-daemon`.

## 문서

- [아키텍처](./ARCHITECTURE.md) — 컴포넌트 구조, 기술 결정, OS별 구현 세부
- [로드맵](./ROADMAP.md) — 단계별 계획 (walking skeleton → 슬라이스 확장)
- [설계 결정 기록](./DESIGN_DECISIONS.md) — 주요 선택의 근거와 대안
- [개발 가이드](./DEVELOPMENT.md) — 로컬 환경 세팅·빌드·테스트
- [API 레퍼런스](./API.md) — HTTP/WebSocket 스펙

## 현재 상태

설계 단계. 범위는 **운영 + 자동화 동등 이원**으로 확정. 구현은 **macOS 우선**(구조는 크로스 플랫폼 유지)으로, 버리는 프로토타입(PoC) 없이 실제 crate 구조 위에서 walking skeleton 부터 착수 예정. 품질 기준은 **일상 개인 사용에 견고한 수준** (자동화는 "견고하거나 없거나").

## 라이선스

*TBD*
</content>
</invoke>
