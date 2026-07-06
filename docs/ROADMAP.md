# Roadmap

본 문서는 `my-supervisor` 프로젝트의 단계별 개발 계획을 큰 카테고리 단위로 정리합니다. 각 단계는 **진입 조건 → 범위 → 완료 조건**으로 구성되며, 완료 조건을 만족해야 다음 단계로 진입합니다.

## 전체 흐름

```
┌─────────┐     ┌──────┐     ┌────────────┐     ┌──────────────────┐
│   PoC   │ ──▶ │ MVP  │ ──▶ │ Production │ ──▶ │ Post-Production  │
│ (1~2주) │     │(3~4주)│     │  (6~8주)   │     │      (지속)       │
└─────────┘     └──────┘     └────────────┘     └──────────────────┘
  기술 검증      개인 실사용    공개 가능 품질        확장 기능
```

---

## 구현 전략 (범위·순서·품질)

본 로드맵의 단계들은 아래 전략 위에서 읽는다 (DD-026 동등 이원, DD-002 호스트 이원).

- **동등 이원 범위**: **운영**(`Process` + `Job`)과 **자동화**(`Rule`)를 동등한 두 기둥으로 만든다. 자동화는 부가 기능이 아니다.
- **macOS 우선, 구조는 크로스플랫폼**: 구현은 macOS 호스트(`desktop`)를 먼저 완성한다. 단 `core`/`application` 과 port 구조는 크로스플랫폼을 유지하고 Linux/Windows 운영 어댑터는 자리만 확보(stub)한다. 윈도우·핫키 자동화는 macOS **단일 구현 seam** (DD-027).
- **버리는 PoC 없음 — walking skeleton 부터**: 던져버릴 프로토타입을 만들지 않는다. 실제 crate 구조 위에서 얇은 수직 슬라이스를 production 품질로 만들고 그대로 확장한다. (아래 "PoC" 단계는 이 의미의 walking skeleton 으로 읽는다.)
- **품질 기준 — 일상 개인 사용에 견고**: 공개 1.0 의 풀 스펙(로그 gzip·메트릭 차트·3-OS 인스톨러 등)은 후순위. **단 자동화(윈도우·핫키)는 "견고하거나 없거나"** — 오작동하는 핫키·윈도우 매니저는 일상 사용에 파괴적이므로 얕게 내지 않는다.
- **슬라이스 순서**:
  1. **① 운영** — 프로세스 + 배치(Job). 호스트 이원(코어 임베드) 위에서.
  2. **② 이벤트 트리거 자동화** — `EventSource`(파일/폴더 watch·타이머) → 프로세스/Job/명령 액션. 크로스플랫폼.
  3. **③ 윈도우·핫키 자동화** — macOS 전용 seam(AX API·event tap·시스템 이벤트) + 권한 모델(DD-029). "견고하거나 없거나".

---

## Phase 1: PoC (Proof of Concept)

**목적:** 아키텍처의 **기술적 리스크를 walking skeleton 으로 먼저 찌른다.** 단 **버리는 코드가 아니라** 실제 crate 구조 위에서 production 품질로 만들어 그대로 확장한다. 이 단계는 호스트 이원(코어 임베드, DD-002)과 **슬라이스 ① 운영** 의 얇은 수직 경로를 세우는 데 집중한다.

### 진입 조건
- 아키텍처 문서 확정
- 모노레포 스켈레톤 생성 — `apps/my_supervisor/{core, application, shared, config, infra/{sqlite,http,logging}, platform/{linux,macos,windows}, daemon, cli, desktop}` 빈 crate 가 workspace 에 등록되어 `cargo check --workspace` 통과

### 범위 (검증 대상)

#### 1. 호스트 이원 구조 (코어 임베드 / 헤드리스 데몬)
- `desktop` (Tauri) 가 `core`/`application` 을 **인프로세스 임베드** + 내장 axum HTTP/WS 서버 기동 (별도 `msv-daemon` spawn 없음 — DD-002)
- 창을 닫아도 tray 상주로 자식 프로세스 유지 → 다시 열면 복귀
- `tauri-plugin-single-instance` 로 앱 이중 실행 방지
- `daemon` (헤드리스) 이 같은 코어를 임베드해 GUI 없이 동일 API 제공 — **두 호스트가 서로 의존하지 않음** 을 확인 (Tauri 는 데몬을 필요로 하지 않음)
- React + Vite 번들 (`apps/my_supervisor/crates/desktop/ui`) 이 Tauri WebView 와 외부 브라우저에서 동일 렌더링·동작
- 호스트 bin 이 타겟 OS 에 따라 `platform/*` 의 `LifecycleController` 구현을 `#[cfg(target_os)]` 로 DI 조립하는 경로 확인

#### 2. `LifecycleController` port 구현 검증 (OS 별)

각 OS crate 가 `core::ports::LifecycleController` 를 구현하고, 공통 spec 테스트 스위트를 통과해야 한다.

- `platform/linux` (`LinuxLifecycle`): `PR_SET_PDEATHSIG` 로 tied 모드, `PR_SET_CHILD_SUBREAPER` 로 손자 reparent 확인
- `platform/windows` (`WindowsLifecycle`): Job Object + `KILL_ON_JOB_CLOSE` 동작 확인, breakaway 플래그로 detached
- `platform/macos` (`MacLifecycle`): shutdown hook + `kqueue` 로 자식 정리 확인, `setsid` 로 detached

spec 테스트는 `application` 레이어에서 trait object 로 작성하여 3 OS 에 동일하게 돌린다.

#### 3. stdout/stderr 스트리밍 + 백프레셔
- 자식이 초당 수만 라인 쏟을 때 데몬 무결성 유지
- bounded channel의 drop 카운트 기록 확인

#### 4. 시그널 핸들링
- `SIGTERM` → grace period → `SIGKILL` 시퀀스
- Windows의 `CTRL_BREAK_EVENT` + `TerminateJobObject` 대체 경로

#### 5. 통신 프로토콜
- axum HTTP 서버 + WebSocket 기본 동작
- CLI 바이너리가 HTTP로 프로세스 목록 조회

### 완료 조건
- 위 5개 항목이 **각각 별도의 테스트 바이너리**로 동작 확인
- Tauri 앱 더블클릭 → 창에서 프로세스 1개 시작 → 창 닫아도 프로세스 유지 → 다시 열면 보임 → CLI에서 `ps`로 동일 프로세스 확인
- 주요 리스크가 이 단계에서 드러나지 않음

### 벗어나야 할 범위
- UI 디자인, UX 완성도
- 설정 파일 포맷 확정
- 로그 로테이션
- 재시작 정책 정교화
- 에러 처리 디테일

### 산출물
- 각 검증용 프로토타입 코드
- 기술 결정을 업데이트한 `ARCHITECTURE.md`
- `API.md` 초안의 검증 결과 및 필요한 보정 사항
- 발견된 이슈 리스트 (MVP에서 처리)

---

## Phase 2: MVP (Minimum Viable Product)

**목적:** 본인이 실제로 쓸 수 있는 수준의 도구를 만든다. 내 평소 프로세스를 이 도구로 관리하면서 1주일 버틸 수 있어야 한다.

### 진입 조건
- PoC의 5개 검증 항목 통과
- 모노레포 최종 구조 확정

### 범위

#### A. 핵심 기능 (Process)
- Port trait 셋 1차 확정 — `LifecycleController`, `ShutdownSignaler`, `StateRepository`, `LogSink`, `ConfigSource`, `Scheduler`, `JobRepository`, `JobRunner`, **`ProcessServiceRegistrar`**. 시그니처 breaking change 는 Phase 3 시작 전까지 허용, 그 이후는 minor 로만.
- `application` Process use case 1차 셋 — `StartProcess`, `StopProcess`, `RestartProcess`(단순 backoff, Direct 전용), `ReloadConfig`, `ConvertManagementMode`
- **Process 관리 모드 이원화 (DD-025)**:
  - `Direct` 모드 완전 구현 — 3 OS 전부
  - `SystemRegistered` 모드 — **사용자 주 사용 OS 1 개** 에 `ProcessServiceRegistrar` 완전 구현 (register/unregister/start/stop/query_status/tail_logs). 나머지 2 OS 는 Phase 3
  - Direct ↔ SystemRegistered 양방향 전환 (`POST /api/v1/processes/{name}/convert`)
  - 재시작 엔진 충돌 회피 — SystemRegistered 에서 `RestartProcess` 는 no-op 반환, `ProcessConfig.restart` 는 unit 파일 생성 시 OS native `Restart=` 지시어로 변환
- TOML 설정 파일 로드·파싱 (`config` crate 의 `TomlConfigSource`)
- 프로세스 CRUD (추가·조회·수정·삭제)
- 프로세스 제어 (start / stop / restart)
- `tied` / `detached` 생명주기 (3 OS `LifecycleController` 구현 완성, Direct 모드에서만 의미)
- `SIGTERM` → grace period → `SIGKILL` 시퀀스 (Unix `ShutdownSignaler`, Windows 는 Phase 3 에서 완성)
- 기본 재시작 정책 (`on-failure` + 단순 backoff, crash loop 는 미포함)

#### A'. Jobs (배치 스케줄러) — 최소 기능
- `application` Job use case — `RegisterJob`, `UpdateJob`, `DeleteJob`, `TriggerJobManual`, `ScheduleTick`, `ObserveJobRunCompleted`, `CancelJobRun`
- Job CRUD
- Trigger 4 종 구현: `cron` (5-field) / `interval` / `one_shot` / `depends_on` (AND + on-success)
- 동시성 정책 `on_overlap: skip | queue | parallel`, 기본 `skip`
- 의존성 트리거 — 등록 시 순환 감지 (`cycle_detected`), 삭제 시 `has_dependents` 검사 + `--force`
- JobRun 이력 — 최근 100 회 유지 (Job 별 `log_retention.max_runs` 기본값)
- 수동 트리거 (`trigger now`)
- `infra/scheduler` crate 구현 — `tokio-cron-scheduler` 기반

#### A''. 자동화 (Rules) — 슬라이스 ②

- `application` Rule use case — `RegisterRule`, `UpdateRule`, `DeleteRule`, `TriggerRuleManual`, `ObserveEvent`
- Rule CRUD
- **이벤트 트리거 (크로스플랫폼)**: `FileChange`(파일/폴더 watch — `EventSource`/`notify`) + 타이머
- **액션**: `StartProcess` / `StopProcess` / `TriggerJob` / `RunCommand` (운영 기둥 연계)
- 수동 시험 발화 (`msv rule run`, Rules 탭)
- RuleFire 이력 (SQLite, 최근 N 회 유지)
- **윈도우·핫키·macOS 시스템 이벤트 트리거/액션은 Phase 3 (슬라이스 ③)** — MVP 는 port(seam) 자리만 확보하고, 미구현 트리거/액션은 등록 거부 (DD-027)

#### B. 로깅
- 프로세스별 로그 파일 (append-only, 로테이션 없음)
- Job Run 별 로그 파일 (run 단위 아카이브, 로테이션 없음)
- 기본 텍스트 포맷 (JSON은 Production에서)

#### C. Desktop (Tauri)
- 앱 실행 → 코어 임베드 + 내장 서버 기동 → WebView 로드 (별도 데몬 spawn 없음)
- 트레이 아이콘 + 기본 메뉴 (열기 / 종료)
- user-level 자동 시작 등록 (`tauri-plugin-autostart`)
- 최소 WebUI:
  - 상위 IA (동등 이원, DD-026): **운영 — Processes · Jobs · Logs** / **자동화 — Rules** / **공통 — Daemon · Settings**
  - 프로세스 목록 (이름, 상태, **관리 모드 뱃지 — Direct / System**, PID 또는 unit, 업타임) + start/stop/restart 버튼 + 프로세스 추가·수정·삭제 폼 (Add Process 폼에 관리 모드 라디오)
  - Process Detail 에 관리 모드 전환 버튼 (Direct ↔ SystemRegistered), 전환 시 진행 상태 및 실패 시 롤백 표시
  - Job 목록 (이름, trigger 요약, 마지막 run, 다음 예정 시각) + Trigger Now 버튼 + Run 이력 테이블 + Job 추가·수정·삭제 폼
  - 로그 뷰어 (최근 N 줄, 프로세스/Job Run 공용). SystemRegistered 프로세스는 journald/launchd logs/Event Log 에서 집계되어 보임
  - **Rule 목록** (이름, 트리거 요약, 마지막 발화, enabled) + 시험 발화 버튼 + Rule 추가·수정·삭제 폼 (트리거: 파일/폴더 watch·타이머 / 액션: 프로세스·Job·명령). 윈도우·핫키 트리거/액션은 Phase 3 (슬라이스 ③)
  - 테마 토글 (auto / dark / light) — 두 모드 동등 지원 (DD-021)

#### D. CLI
- `msv ps` (관리 모드 컬럼 포함)
- `msv start/stop/restart <n>`
- `msv logs <n> [-f]`
- `msv convert <n> --to direct` / `--to system --unit-name <name>` (관리 모드 전환)
- `msv daemon start/stop/status`
- `msv jobs ls`
- `msv jobs add -c job.toml`
- `msv jobs trigger <name>`
- `msv jobs runs <name> [--limit 20]`
- `msv jobs logs <name> <run-id> [-f]`
- `msv jobs rm <name> [--force]`
- `msv rule ls` / `msv rule add -c rule.toml` / `msv rule run <name>` / `msv rule rm <name>` (자동화 — 슬라이스 ②)
- JSON 출력 (`-o json`) 기본 지원

#### E. 배포
- 3개 OS 중 **1개**에 대해 설치 프로그램 프로토타입 (본인 주 사용 OS)
- 나머지는 `cargo build` 수준

### 완료 조건
- 본인의 평소 프로세스 2~3 개 + Job 1 개 이상을 my-supervisor 로 대체하여 **1주일 연속 무탈 운영**
- **이벤트 트리거 Rule 1 개 이상** (예: 폴더 변경 → 프로세스 재시작/명령 실행) 등록·동작 (슬라이스 ②, DD-026)
- 그중 **최소 1 개 프로세스는 `management_mode = SystemRegistered`** 로 운영 (사용자 주 사용 OS 에서)
- Direct ↔ SystemRegistered 양방향 전환 시나리오 성공 (convert API + CLI 양쪽)
- 설치·사용 중 명확한 블로커가 없음
- Server 배포를 가정한 최소 시나리오 통과: 별도 머신에 데몬(`msv-daemon`)+CLI(`msv`)만 복사해서 `msv add` → `msv start` 로 프로세스 1 개, `msv jobs add` → `msv jobs trigger` 로 Job 1 개 관리 성공

### 벗어나야 할 범위
- Crash loop 감지
- 로그 로테이션·압축
- 구조화 로깅
- 헬스체크
- 시스템 서비스 통합
- 알림
- 리소스 제한
- 좀비/고아 reconciliation
- Job OR 의존성·on-failure·on-any
- Job DAG 비주얼라이저
- Job 백필 (missed-runs 재실행)
- Job pause/resume, templates

### 산출물
- 실사용 가능한 데몬·CLI·Desktop 앱
- 기본 설치 스크립트
- 사용 중 발견한 UX·버그 리스트 (Production 입력값)

---

## Phase 3: Production

**목적:** 내가 아닌 **다른 개인 개발자에게도 권할 수 있는** 품질에 도달한다. 공개 가능한 1.0 릴리즈를 지향한다.

### 진입 조건
- MVP 완료
- 실사용 중 발견된 주요 UX 문제 리스트 확보

### 범위

#### A. 재시작·안정성
- Exponential backoff (설정 가능 파라미터)
- Crash loop 감지 + 알림
- 재시작 히스토리 저장 (SQLite)
- 프로세스 상태 머신 정비 (Starting/Running/Stopping/Crashed/Stopped)

#### B. 로깅 완성
- JSON 구조화 로그
- 크기·시간 기반 로테이션
- gzip 압축 + 오래된 파일 자동 삭제
- `SIGUSR1`로 로그 파일 재오픈 (logrotate 연동)
- WebUI 로그 tail (WebSocket + follow 모드 + rate limit)

#### C. 프로세스 신원·회수
- 신원 확인 3종 세트 (PID + start_time + UUID 태그)
- `PR_SET_CHILD_SUBREAPER` 적용 (Linux)
- Reconciliation 루프 (10초 주기)
- 좀비 즉시 수거 (`waitpid WNOHANG`)
- Write-ahead state 영속화

#### D. 시그널·Shutdown
- `SIGHUP` 설정 리로드
- `SIGUSR1` 로그 재오픈
- `SIGPIPE` 무시
- Windows graceful shutdown 경로 완성 (`CTRL_BREAK_EVENT` + Named pipe 옵션)
- Shutdown 진행 상황 로깅

#### E. 시스템 서비스 통합 (Opt-in) — 두 축

**(A) 데몬 자체 기동 모드** — `AutoStartService` port 최종 확정 + 3 OS adapter 완성:
- `platform/linux` · `SystemdUserUnit` — `~/.config/systemd/user/my-supervisor.service` 생성·enable·disable
- `platform/macos` · `LaunchdAgent` — `~/Library/LaunchAgents/com.my-supervisor.daemon.plist` 생성·bootstrap
- `platform/windows` · `TaskSchedulerEntry` 또는 `WindowsService` 등록
- UI·CLI 에서 one-click 토글 (`msv autostart enable/disable`)

**(B) 관리 대상 프로세스 관리 모드 — SystemRegistered 나머지 2 OS 완성** (MVP 에서 주 사용 OS 1 개는 이미 완성):
- `platform/linux` · `SystemdUserUnitProcess` — `~/.config/systemd/user/my-supervisor-managed-<name>.service`
- `platform/macos` · `LaunchdAgentProcess` — `~/Library/LaunchAgents/com.my-supervisor.managed.<name>.plist`
- `platform/windows` · `WindowsServiceProcess` — `sc create/delete/start/stop`
- **유닛 잔재 정리 커맨드**: `msv repair system-registrations` — my-supervisor 네임스페이스 prefix 를 기준으로 잔재 unit 스캔·정리
- 전환 실패 복구 UX — `service_registration_failed` 발생 시 UI 에 원인·복구 제안 표시

각 OS 네이티브 로깅 `LogSink` 보조 구현 (journald / launchd logs / Event Log) 도 이 단계에 완성. Direct·SystemRegistered 양쪽에 적용.

#### F. 헬스체크
- HTTP / TCP / exec 3종
- 실패 임계 초과 시 재시작 트리거 옵션

#### G. 영속화·메트릭
- SQLite 스키마 확정 (processes, state_history, restart_history, metrics)
- WebUI 대시보드에 CPU/메모리 시계열 차트
- 재시작 히스토리 UI

#### H. 알림
- 네이티브 데스크톱 알림 (`tauri-plugin-notification`)
- 트리거: crash loop 진입, 헬스체크 실패, 프로세스 장기 미기동, Job run 실패, Job run 장시간 대기 (queue 병목)

#### H'. Jobs 확장
- OR 의존성 시맨틱 (`{ type: "depends_on_any", jobs: […] }`)
- `on_dependency_failure: run_anyway` 외에 `trigger_on_failure` 추가 (실패를 트리거로 사용)
- Job DAG 비주얼라이저 (upstream·downstream 그래프, 병목 하이라이트)
- 백필 (missed-runs) — 데몬 다운 구간의 cron/interval 예약된 run 을 재기동 시 사용자 선택으로 재실행
- Job pause / resume (스케줄 일시 정지)
- Job run 아카이브 로그를 별도 디렉터리로 ( `~/.local/share/my-supervisor/job-runs/<name>/<run-id>/`)
- Job templates / cloning

#### H''. 자동화 확장 (Rules) — 슬라이스 ③ (윈도우·핫키, macOS)

- macOS 단일 구현 seam 완성 (DD-027): `WindowAutomation`(AX API), `HotkeyBinder`(event tap), `SystemEventWatcher`(NSWorkspace/IOKit)
- Rule 트리거에 `SystemEvent` · `Hotkey` 추가, 액션에 `Window`(이동·리사이즈·레이아웃) 추가
- **권한 모델 (DD-029)**: Accessibility / Input Monitoring 권한 요청·상태 표시·graceful degradation (`enabled = false (permission_required)`)
- **"견고하거나 없거나"**: 윈도우·핫키는 얕게 내지 않는다. 오작동 시 사용자 작업을 방해하므로 견고하게 완성하거나 비활성으로 둔다
- 비-macOS 호스트는 이 영역 비활성 (등록 거부 유지)

#### I. 배포 완성
- 3개 OS 설치 프로그램 (Windows MSI, macOS DMG, Linux deb + AppImage)
- Server용 `curl \| sh` 설치 스크립트
- Docker 이미지 (서버 배포용)
- 자동 업데이트 메커니즘 검토 *(구현은 Post-Production)*

#### J. 문서
- 사용자 가이드 (설치·시작하기·설정 레퍼런스)
- API 문서 최종화 (`docs/API.md` — PoC 초안 기반, 구현 반영)
- 트러블슈팅 가이드
- 기여 가이드 (오픈소스화 시)

### 완료 조건
- 3개 OS에서 정상 설치·동작
- 공개 저장소에 1.0 릴리즈 태깅 가능한 품질
- 외부 사용자 최소 1명이 설치·사용하여 중요한 버그 없이 1주일 운영
- `ARCHITECTURE.md`와 실제 구현이 일치

### 벗어나야 할 범위
- 리소스 제한 (메모리 쿼터 등)
- 설정 hot reload
- 로그 검색
- 원격 데몬 제어 UX
- 비-systemd Linux 지원 (OpenRC 등)

### 산출물
- 1.0 릴리즈 바이너리 (3개 OS)
- 공개 문서
- 설치·시작 튜토리얼

---

## Phase 4: Post-Production

**목적:** 1.0 공개 이후 요구되는 기능을 **점진적으로** 추가한다. 엄격한 순서 없음.

### 범위 (우선순위 순, 유동적)

#### 1. 리소스 제한
- 메모리 한도 + OOM 재시작 (Linux cgroup v2, Windows Job Object)
- CPU 쿼터 *(수요 있으면)*

#### 2. 설정 Hot Reload
- `notify` 크레이트로 설정 파일 watch
- 변경 시 영향 받은 프로세스만 재시작
- `SIGHUP`과 동일 효과지만 자동화

#### 3. 로그 검색·분석
- SQLite FTS5로 전문 검색
- WebUI에서 프로세스·시간 범위·키워드 필터

#### 4. 원격 관리
- SSH 터널링 UX (Tauri 앱에서 원격 데몬 연결)
- 인증 레이어 (API 키 또는 OAuth)
- 원격 시 바인딩을 `127.0.0.1` 제약 완화

#### 5. 자동 업데이트
- Tauri updater 통합
- 데몬 바이너리 무중단 교체 전략 검토

#### 6. 추가 init 시스템 지원
- OpenRC (Alpine Linux) — 수요 있으면
- runit / s6 — 컨테이너 환경 수요 있으면

#### 7. 확장성
- 플러그인 시스템 검토 (WASM 기반?)
- Webhook 발송 (crash 시 외부 알림)
- Prometheus 메트릭 엔드포인트

#### 8. 고급 UX
- 프로세스 그룹화·라벨
- 일괄 작업 (전체 재시작 등)
- 설정 템플릿·스니펫
- 다국어

---

## 리스크 레지스터

단계별로 경계해야 할 리스크를 한 곳에 모은 목록입니다. 각 단계 시작 시 관련 항목 재확인.

| ID | 리스크 | 영향 단계 | 완화 방법 |
|---|---|---|---|
| R1 | Tauri WebView 버전 차이 (WebView2 / WKWebView / WebKitGTK) | PoC | 초기부터 3개 OS에서 테스트 |
| R2 | Windows Job Object의 자식 탈출 케이스 | PoC, Production | breakaway 플래그 검증, 테스트 케이스 축적 |
| R3 | macOS의 `PR_SET_PDEATHSIG` 부재 | PoC | shutdown hook 방식 초기 채택 |
| R4 | stdout 백프레셔로 자식 블록 | PoC, MVP | bounded channel + drop 정책 초기 적용 |
| R5 | Crash loop이 CPU 100% 유발 | MVP, Production | backoff + 임계값 필수 |
| R6 | PID 재사용으로 인한 오탐지 | Production | 신원 확인 3종 세트 |
| R7 | 로그 파일 무한 증가 | Production | 로테이션·삭제 정책 기본값 보수적으로 |
| R8 | 시스템 서비스 등록 실패 / 권한 문제 | Production | 사전 진단 메시지, rollback 경로 |
| R11 | cron 평가 타이머 정확도 (tokio 기반이라 ±1 s 오차) | MVP | "배치 워크로드 기준 허용치" 로 명시, 초당 정밀도 필요 시 사용자 안내 |
| R12 | Job 의존 그래프 순환 등록 — 런타임 deadlock 가능 | MVP | **등록 시** DFS 기반 순환 감지, 감지 시 `422 cycle_detected` 로 거부 (DD-024) |
| R13 | daemon 다운 중 예약된 Job run 누락 | MVP → Production | MVP 는 감수, Phase 3 에 백필 기능 추가 (H' 참조) |
| R14 | Job run 로그 무한 누적 | MVP | `log_retention.max_runs` 기본 100, `max_age_days` 옵션; Phase 3 아카이브 디렉터리 분리 |
| R15 | SystemRegistered 유닛 잔재 — `unregister` 실패 시 OS 에 고아 unit 파일 누적 | MVP, Production | 전환은 atomic (실패 시 롤백), Phase 3 에 `msv repair system-registrations` CLI. 네임스페이스 prefix 로 스캔 |
| R16 | 재시작 엔진 이중 발화 — 데몬 backoff + OS `Restart=` 가 동시 동작하면 상태 깨짐 | MVP | `management_mode = SystemRegistered` 에서 `RestartProcess` use case 를 no-op 로 강제 (DD-025), 테스트로 회귀 방지 |
| R9 | SQLite 락 경합 (많은 프로세스 + 잦은 이벤트) | Production | WAL 모드, 배치 쓰기 |
| R10 | 데몬 크래시 시 관리 중이던 자식의 고아화 | Production | subreaper + write-ahead state |
| R17 | 자동화 권한 미부여 — AX/핫키 권한 없으면 윈도우·핫키 Rule 무동작 | MVP→Production | graceful degradation: `enabled = false (permission_required)` + UI 상태·설정 안내, 조용한 실패 금지 (DD-029) |
| R18 | 윈도우 자동화 누수 — AX API 가 앱·Space·디스플레이 변화에 취약 | Production | "견고하거나 없거나" 원칙, macOS 단일 구현으로 케이스 집중, 실패 시 명확한 사유 노출 |
| R19 | 글로벌 핫키 충돌 — 다른 앱·OS 단축키와 겹침 | Production | 등록 시 충돌 감지·경고, 사용자 재바인딩 UX |
| R20 | 비-macOS 호스트에 윈도우/핫키 Rule 등록 시도 | MVP→Production | 등록 시점 `not_supported_on_platform` 거부, seam 미주입(`None`) (DD-027) |

---

## 측정 지표 (Success Metrics)

각 단계 진척을 정성적이 아닌 정량적으로 판단하기 위한 지표.

### PoC
- 5개 기술 검증 항목의 pass/fail

### MVP
- 본인 실사용 연속 일수 (목표: 7일) — Process 2~3 + Job 1 개 이상 포함
- 외부 도움 없이 설치→첫 프로세스 기동까지 시간 (목표: 5분 이내)
- Job 등록 → 첫 trigger 실행까지 지연 (목표: 500ms 이내, cron 평가 오차 ±1s)

### Production
- 3개 OS 설치 성공률 100%
- 외부 베타 사용자 수 (목표: 3명)
- 1주일 연속 운영 시 데몬 자체 크래시 0건
- 프로세스 1000개 관리 시 데몬 메모리 사용량 (목표: 100MB 이하)
- 프로세스 재시작 지연 (목표: 100ms 미만)

### Post-Production
- 각 기능 요청 이슈 처리량
- 커뮤니티 기여자 수 *(오픈소스화 시)*

---

## 버전 체계

Semantic Versioning 기반:

- `0.1.x` — PoC 종료 시점 내부 태그
- `0.5.x` — MVP 완료 시점
- `1.0.0` — Production 단계 완료
- `1.x.x` — Post-Production 기능 추가 (minor), 버그 수정 (patch)
- `2.0.0` — 호환성 파괴 변경 발생 시 *(현 시점 계획 없음)*
