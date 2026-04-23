# my-supervisor

> 크로스 플랫폼 프로세스 매니저. 데스크톱 GUI가 본서비스, CLI/WebUI가 확장.

## 개요

`my-supervisor` (CLI 명령어 `msv`) 는 개인 개발자와 소규모 운영 환경을 위한 **프로세스 + 배치(cron) 통합 관리 도구**입니다. PM2 처럼 장시간 실행 프로세스를 감시·재시작하고, cron 처럼 예약·주기·의존성 기반 배치 Job 도 같은 UI 에서 관리합니다. **GUI 우선**으로 설계되어 설정과 운영이 쉽고, 서버 환경에서는 **CLI 와 WebUI** 로 동일한 경험을 제공합니다.

### 핵심 가치

- **GUI 우선**: Tauri 기반 네이티브 데스크톱 앱. 설정·모니터링·로그 확인이 전부 클릭 단위.
- **크로스 플랫폼**: Windows / macOS / Linux 동일하게 동작.
- **Process + Job 이원 모델**: 장시간 실행 프로세스와 cron/interval/one-shot/의존성 기반 Job 을 **하나의 GUI** 에서 관리. Job 스케줄링은 OS crontab · Task Scheduler 에 위임하지 않고 데몬이 자체 실행.
- **테마 동등 지원**: 다크/라이트 모드를 1 급 지원 (shadcn/ui CSS 변수 기반).
- **Production 품질**: 로그 로테이션, crash loop 감지, 좀비 처리, graceful shutdown 등 production 기본기 내장.
- **서버 친화**: 헤드리스 환경에서도 CLI + WebUI 로 동일한 제어 가능.
- **시스템 통합**: systemd / launchd / Windows Service 와 opt-in 연동 (데몬 자체의 자동 시작 — Job 과는 별개).

## 아키텍처 한눈에

```
┌──────────────┐  ┌──────────────┐  ┌─────────────┐
│  Tauri 앱    │  │   브라우저    │  │    msv      │
│ (데스크톱용) │  │ (서버 SSH 시) │  │   (CLI)     │
└──────┬───────┘  └──────┬───────┘  └──────┬──────┘
       │                 │                 │
       └─────────────────┼─────────────────┘
                         │ HTTP / WebSocket (localhost)
                  ┌──────▼───────┐
                  │  msv-daemon  │  ← 실제 supervisor
                  └──────┬───────┘
                         │ spawn / signal / monitor
                  ┌──────▼───────┐
                  │ 자식 프로세스 │
                  └──────────────┘
```

데몬은 독립 바이너리이고, Tauri 앱·CLI·브라우저는 모두 같은 HTTP API를 소비하는 클라이언트입니다. UI를 닫아도 데몬과 자식 프로세스는 유지됩니다.

내부 모듈은 Hexagonal (Ports & Adapters) 로 분리되어 있습니다. 도메인 로직은 OS·DB·HTTP 세부를 모르고, 플랫폼·인프라 adapter crate 가 port trait 를 구현합니다.

```
  app/*  ──▶ application ──▶ core  ◀── port trait 정의
    │             │             ▲
    │             └── shared ───┤  (DTO 재사용)
    │                           │
    ├──▶ infra/*    ─────────── │   adapter 구현
    ├──▶ platform/* ─────────── │   adapter 구현
    └──▶ config     ─────────── ┘
```

상세는 [ARCHITECTURE.md §3](./ARCHITECTURE.md#3-모노레포-구조) · [DESIGN_DECISIONS.md DD-017~DD-020](./DESIGN_DECISIONS.md#dd-017-hexagonal-아키텍처-ports--adapters-채택) 참조.

## 배포 형태

| 배포 | 타깃 | 구성 |
|---|---|---|
| Desktop | 개인 개발자, 로컬 작업 | Tauri 앱 + 데몬 + CLI |
| Server | 서버, WSL, CI, 컨테이너 | 데몬 + CLI (Tauri 없음) |

## 기술 스택

- **언어**: Rust (전 컴포넌트)
- **UI 셸**: Tauri v2
- **데몬**: tokio + axum
- **프론트엔드**: React + Vite + Tailwind + shadcn/ui (`packages/ui`, feature 단위 모듈 — DD-020). 상위 IA 5 개: **Processes · Jobs · Logs · Daemon · Settings**. 다크/라이트 양립.
- **영속화**: TOML (설정) + SQLite (런타임 상태 · JobRun 이력)
- **배치 스케줄러**: 데몬 내장 (`infra/scheduler`, `tokio-cron-scheduler` 기반). cron 5-field + interval + one-shot + 의존성.
- **모듈 구조**: Hexagonal 5-레이어 Cargo workspace — `core` / `application` / `shared`·`config` / `infra/*` / `platform/*` / `app/*`. Cargo 패키지 prefix `my-supervisor-`, 바이너리는 `msv` · `msv-daemon`.

## 문서

- [아키텍처](./ARCHITECTURE.md) — 컴포넌트 구조, 기술 결정, OS별 구현 세부
- [로드맵](./ROADMAP.md) — PoC → MVP → Production 단계별 계획
- [설계 결정 기록](./DESIGN_DECISIONS.md) — 주요 선택의 근거와 대안
- [개발 가이드](./DEVELOPMENT.md) — 로컬 환경 세팅·빌드·테스트
- [API 레퍼런스](./API.md) — HTTP/WebSocket 스펙 (설계 레벨 초안)

## 현재 상태

설계 단계. PoC 착수 예정.

## 라이선스

*TBD*
