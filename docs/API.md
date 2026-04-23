# API Reference

`msv-daemon` (crate: `my-supervisor-app-daemon`) 이 외부 클라이언트(데스크톱 Tauri 앱, 외부 브라우저, `msv` CLI, 스크립트) 에게 제공하는 HTTP / WebSocket API 를 정리합니다. 내부적으로는 `crates/infra/http` adapter 가 `core::ports::HttpServer` 를 구현하여 이 엔드포인트를 호스팅합니다.

> **현재 문서 상태**: **설계 레벨 스펙 초안**. 리포지토리는 아직 PoC 단계 이전이며, 본 문서는 `ARCHITECTURE.md` §5를 기반으로 엔드포인트·오류 포맷·타입을 선정의한 것입니다. request/response 세부 필드는 PoC·MVP 구현 시 확정되며, 구현 이후 Phase 3 §J에서 최종화합니다.

관련 문서: [아키텍처](./ARCHITECTURE.md) · [설계 결정](./DESIGN_DECISIONS.md) · [로드맵](./ROADMAP.md)

---

## 1. 원칙

- **바인딩**: `127.0.0.1:<port>` (기본 9876). `0.0.0.0` 바인딩은 코드 레벨에서 금지 (DD-011).
- **인증**: 없음 (단일 유저·로컬 전용 전제). 원격 필요 시 Post-Production에서 인증 레이어 추가.
- **Prefix**: 모든 REST 엔드포인트는 `/api/v1/` 하위. breaking change는 `/api/v2/` 신설로 처리.
- **Content-Type**: 요청·응답 모두 `application/json` (UTF-8).
- **타입 출처**: `crates/shared` (패키지 `my-supervisor-shared`) 의 Rust 타입 (`serde` 직렬화) 과 1:1 대응. 구체 경로는 `crates/shared/src/api.rs` (REST), `crates/shared/src/events.rs` (WS 이벤트), `crates/shared/src/config.rs` (TOML 스키마). 본 문서의 스키마는 그 서브셋을 기술적 설명용으로 재표기한 것.
- **CLI↔데몬 전용 경로**: Unix domain socket (`~/.local/state/my-supervisor/daemon.sock`) 도 동일한 API를 제공할 수 있음 (설정 시). 프로토콜 의미는 동일.

---

## 2. REST 엔드포인트

### 2.1 프로세스 리소스

| 메서드 | 경로 | 설명 |
|---|---|---|
| `GET` | `/api/v1/processes` | 관리 중인 프로세스 목록 |
| `POST` | `/api/v1/processes` | 프로세스 등록 (body: `ProcessConfig`) |
| `GET` | `/api/v1/processes/{name}` | 프로세스 상세 |
| `DELETE` | `/api/v1/processes/{name}` | 프로세스 등록 해제 (실행 중이면 409) |
| `POST` | `/api/v1/processes/{name}/start` | 시작 |
| `POST` | `/api/v1/processes/{name}/stop` | 중지 (graceful → force) |
| `POST` | `/api/v1/processes/{name}/restart` | 재시작 (crash loop 카운터 리셋) |
| `GET` | `/api/v1/processes/{name}/logs` | 최근 로그. 쿼리: `tail`, `since` |

#### GET /api/v1/processes

응답 예시:

```json
{
  "processes": [
    {
      "name": "api-server",
      "state": "running",
      "pid": 12345,
      "restart_count": 0,
      "started_at": "2026-04-21T09:00:00Z",
      "cpu_percent": 1.2,
      "memory_bytes": 48234000
    }
  ]
}
```

- 상태 코드: `200 OK`.
- 정렬: 응답의 `processes`는 이름 오름차순.

#### POST /api/v1/processes

요청 바디: `ProcessConfig` (§4.3 참조). 설정 파일의 `[[process]]` 한 개 항목과 동일한 형태.

- `201 Created` — 등록 성공. 응답 바디는 `ProcessStatus`.
- `400 invalid_config` — 필수 필드 누락, 값 제약 위반.
- `409 name_conflict` — 같은 이름이 이미 등록됨.

#### GET /api/v1/processes/{name}

응답: `ProcessStatus` (§4.1).

- `200 OK` / `404 process_not_found`.

#### DELETE /api/v1/processes/{name}

- `204 No Content` — 제거 완료.
- `404 process_not_found`.
- `409 already_running` — 실행 중에는 거부. `?force=true` 시 graceful stop 후 제거(구현 시 확정).

#### POST /api/v1/processes/{name}/start

- `202 Accepted` — 시작 시퀀스 개시. 상태 전이는 WebSocket `process.state_changed`로 관찰.
- `404 process_not_found`.
- `409 already_running`, `409 crash_loop_detected`.

#### POST /api/v1/processes/{name}/stop

쿼리(선택): `?force=true`면 grace_period 생략.

- `202 Accepted`.
- `404 process_not_found`, `409 not_running`.

#### POST /api/v1/processes/{name}/restart

- `202 Accepted`. crash loop 상태여도 이 호출은 카운터를 리셋.
- `404 process_not_found`.

#### GET /api/v1/processes/{name}/logs

쿼리:

| 파라미터 | 기본값 | 설명 |
|---|---|---|
| `tail` | `100` | 반환할 최근 라인 수 (최대 10000) |
| `since` | *(생략 시 무제한)* | RFC3339 타임스탬프. 해당 시각 이후 로그만 |

응답 예시:

```json
{
  "lines": [
    {
      "timestamp": "2026-04-21T10:30:45.123Z",
      "stream": "stdout",
      "line": "Server listening on :3000"
    }
  ],
  "truncated": false,
  "dropped_count": 0
}
```

- `truncated`: `tail`/`since` 필터로 잘려나간 라인이 있는지
- `dropped_count`: 백프레셔로 버려진 라인 수 (DD-012)

### 2.2 데몬 리소스

| 메서드 | 경로 | 설명 |
|---|---|---|
| `GET` | `/api/v1/daemon/status` | 데몬 상태 |
| `POST` | `/api/v1/daemon/reload` | 설정 파일 리로드 (SIGHUP과 동등) |
| `POST` | `/api/v1/daemon/shutdown` | 데몬 종료 (graceful shutdown 시퀀스 시작) |

#### GET /api/v1/daemon/status

```json
{
  "version": "0.1.0",
  "started_at": "2026-04-21T08:00:00Z",
  "pid": 9876,
  "process_count": 3,
  "config_path": "/home/user/.config/my-supervisor/config.toml",
  "log_dir": "/home/user/.local/share/my-supervisor/logs"
}
```

#### POST /api/v1/daemon/reload

- `202 Accepted` — 리로드 시작.
- `400 invalid_config` — 리로드 대상 설정이 유효하지 않음. 데몬은 기존 설정 유지.

#### POST /api/v1/daemon/shutdown

- `202 Accepted`. 자식 프로세스의 생명주기 모드(`tied` / `detached`)에 따라 자식 정리 후 종료.

### 2.3 Jobs 리소스

`ARCHITECTURE.md §12` (Jobs 배치 스케줄러) 의 wire 인터페이스. Job 등록·수정·삭제 + 수동 트리거 + Run 이력 조회가 한 set.

| 메서드 | 경로 | 설명 |
|---|---|---|
| `GET` | `/api/v1/jobs` | 등록된 Job 목록 |
| `POST` | `/api/v1/jobs` | Job 등록 (body: `JobConfig`) |
| `GET` | `/api/v1/jobs/{name}` | Job 상세 |
| `PATCH` | `/api/v1/jobs/{name}` | Job 수정 (부분 업데이트) |
| `DELETE` | `/api/v1/jobs/{name}` | Job 제거 (쿼리 `?force=true` 로 downstream 의존 해제) |
| `POST` | `/api/v1/jobs/{name}/trigger` | 수동 즉시 실행 (trigger 타입과 무관) |
| `GET` | `/api/v1/jobs/{name}/runs` | Run 이력. 쿼리: `limit`, `since`, `state` |
| `GET` | `/api/v1/jobs/{name}/runs/{run_id}` | Run 상세 |
| `POST` | `/api/v1/jobs/{name}/runs/{run_id}/cancel` | 진행 중 Run 중단 |
| `GET` | `/api/v1/jobs/{name}/runs/{run_id}/logs` | Run 로그 (REST 일회성). 쿼리: `tail`, `since` |

#### GET /api/v1/jobs

응답 예시:

```json
{
  "jobs": [
    {
      "name": "nightly-backup",
      "trigger": { "type": "cron", "expr": "0 2 * * *" },
      "on_overlap": "skip",
      "last_run": {
        "run_id": "01JXYZ…",
        "state": "succeeded",
        "ended_at": "2026-04-23T02:00:31Z",
        "duration_sec": 31
      },
      "next_run_at": "2026-04-24T02:00:00Z",
      "success_rate_recent": 0.95,
      "dependencies": { "upstream": [], "downstream": ["post-backup-verify"] }
    }
  ]
}
```

- 상태 코드: `200 OK`
- 정렬: `jobs` 는 이름 오름차순

#### POST /api/v1/jobs

요청 바디: `JobConfig` (§4.4). 설정 파일의 `[[job]]` 한 항목과 동일한 형태.

- `201 Created` — 등록 성공. 응답 바디는 `JobStatus`
- `400 invalid_request` — 바디 파싱 실패
- `400 invalid_cron_expression` — `trigger.type = "cron"` 이고 `expr` 문법 오류
- `409 job_name_conflict` — 같은 이름이 이미 등록됨
- `422 cycle_detected` — `trigger.type = "depends_on"` 이 순환을 만듦

#### PATCH /api/v1/jobs/{name}

- `200 OK`. 응답은 `JobStatus`
- `404 job_not_found` / `400 invalid_cron_expression` / `422 cycle_detected`

#### DELETE /api/v1/jobs/{name}

- `204 No Content` — 제거 완료
- `404 job_not_found`
- `409 has_dependents` — downstream 의존 Job 존재. `?force=true` 시 의존 해제 후 제거

#### POST /api/v1/jobs/{name}/trigger

- `202 Accepted` — Run 생성 시작. 응답 헤더 `Location: /api/v1/jobs/{name}/runs/{run_id}` 로 새 Run 경로 제공. 실행 진행은 WS 로 관찰
- `404 job_not_found`
- `409 already_running` — `on_overlap = "skip"` 이고 진행 중 Run 존재
- `409 queued` — `on_overlap = "queue"` 이고 진행 중 Run 있어 대기열 삽입 (응답 바디에 대기 순번 포함)

#### GET /api/v1/jobs/{name}/runs

쿼리:

| 파라미터 | 기본값 | 설명 |
|---|---|---|
| `limit` | `50` | 반환 개수 (최대 500) |
| `since` | *(생략 시 제한 없음)* | RFC3339. 해당 시각 이후 시작된 Run |
| `state` | *(생략 시 전부)* | `pending` / `running` / `succeeded` / `failed` / `cancelled` / `skipped` 중 하나 |

응답:

```json
{
  "runs": [
    {
      "run_id": "01JXYZ…",
      "job_name": "nightly-backup",
      "triggered_by": { "type": "schedule" },
      "scheduled_at": "2026-04-23T02:00:00Z",
      "started_at": "2026-04-23T02:00:00.124Z",
      "ended_at": "2026-04-23T02:00:31.007Z",
      "exit_code": 0,
      "state": "succeeded"
    }
  ],
  "truncated": false
}
```

`triggered_by` 타입: `schedule` / `manual` / `dependency` (후자는 `{ "type": "dependency", "upstream_run_id": "…" }` 형태).

#### POST /api/v1/jobs/{name}/runs/{run_id}/cancel

- `202 Accepted` — 중단 시퀀스 개시 (`ShutdownSignaler` 호출)
- `404 run_not_found`
- `409 run_already_finished` — 이미 종료된 Run

---

## 3. WebSocket 엔드포인트

| 경로 | 설명 |
|---|---|
| `/api/v1/events` | 전역 이벤트 스트림 |
| `/api/v1/processes/{name}/logs` | 특정 프로세스 로그 follow |
| `/api/v1/jobs/{name}/runs/{run_id}/logs` | 특정 Run 로그 follow |

### 3.1 /api/v1/events

메시지 포맷:

```json
{
  "type": "process.state_changed",
  "timestamp": "2026-04-21T10:30:45.123Z",
  "payload": { ... }
}
```

이벤트 타입:

| `type` | `payload` 요약 |
|---|---|
| `process.state_changed` | `{ name, from, to }` — `ProcessState` 전이 |
| `process.crashed` | `{ name, exit_code, signal, restart_count }` |
| `process.crash_loop_detected` | `{ name, window_sec, threshold }` |
| `process.health_check_failed` | `{ name, check_type, failure_count }` |
| `job.registered` / `job.updated` / `job.deleted` | `{ name }` (updated 는 변경 필드 diff 포함) |
| `job.run_scheduled` | `{ name, run_id, scheduled_at, triggered_by }` |
| `job.run_started` | `{ name, run_id, started_at, pid }` |
| `job.run_succeeded` | `{ name, run_id, ended_at, duration_sec, exit_code }` |
| `job.run_failed` | `{ name, run_id, ended_at, duration_sec, exit_code }` |
| `job.run_skipped` | `{ name, run_id, reason }` — `reason`: `overlap_skip` / `dependency_failure` / `dependency_skip` |
| `job.run_cancelled` | `{ name, run_id, cancelled_by }` |

### 3.2 /api/v1/processes/{name}/logs

접속 시 실시간 로그 라인을 스트리밍. 포맷은 REST `/logs`의 `lines` 요소와 동일.

- **Rate limit**: 초당 라인 상한 초과 시 `{ "type": "log.dropped", "payload": { "count": N } }` 제어 프레임을 삽입 (DD-012).
- 연결 종료: 클라이언트가 close, 또는 프로세스 등록 해제 시 서버가 close frame + `code` 전송.

### 3.3 /api/v1/jobs/{name}/runs/{run_id}/logs

해당 Run 의 stdout/stderr 라인을 실시간 스트리밍. 포맷·rate limit·drop 규칙은 §3.2 와 동일. Run 종료 시 서버가 close frame 으로 종료(정상·실패 여부는 `/events` 의 `job.run_*` 메시지 참조).

### 3.4 공통 사항

- 하위 프로토콜은 없으며 텍스트 프레임에 JSON.
- 서버가 오류로 연결을 닫을 때 close frame의 reason에 `error.code`를 담는다 (`ARCHITECTURE.md` §5.4 참조).

---

## 4. 공용 타입

실제 스키마는 `crates/shared` 의 Rust 타입을 source of truth 로 한다 — 구체적으로 `crates/shared/src/api.rs` (REST DTO), `crates/shared/src/events.rs` (WS 이벤트), `crates/shared/src/config.rs` (TOML 설정 스키마). 아래는 설계 단계 레퍼런스.

### 4.1 ProcessStatus

```ts
interface ProcessStatus {
  name: string;
  state: ProcessState;
  pid: number | null;
  restart_count: number;
  started_at: string | null;   // RFC3339
  cpu_percent: number;
  memory_bytes: number;
}
```

### 4.2 ProcessState

```ts
type ProcessState =
  | "starting"
  | "running"
  | "stopping"
  | "crashed"
  | "stopped";
```

### 4.3 ProcessConfig (요약)

`ARCHITECTURE.md` §15 설정 파일 예시의 `[[process]]` 블록과 대응.

```ts
interface ProcessConfig {
  name: string;
  command: string;
  args?: string[];
  cwd?: string;
  env?: Record<string, string>;
  lifecycle?: "tied" | "detached";   // 기본 "tied"
  autostart?: boolean;
  restart?: RestartPolicy;
  shutdown?: ShutdownPolicy;
  health_check?: HealthCheck;
  logging?: LoggingPolicy;
}
```

하위 객체의 필드는 `ARCHITECTURE.md` §15 / §7 / §8 / §9 / §13 과 일치한다.

### 4.4 JobConfig · JobStatus · JobRun

`ARCHITECTURE.md` §12 Jobs 섹션 및 §15 설정 예시의 `[[job]]` 블록과 대응.

```ts
interface JobConfig {
  name: string;
  command: string;
  args?: string[];
  cwd?: string;
  env?: Record<string, string>;
  trigger: JobTrigger;
  on_overlap?: "skip" | "queue" | "parallel";            // 기본 "skip"
  on_dependency_failure?: "skip" | "run_anyway";         // 기본 "skip"
  timeout_sec?: number;
  log_retention?: { max_runs?: number; max_age_days?: number };
}

type JobTrigger =
  | { type: "cron";       expr: string }                 // 5-field
  | { type: "interval";   every_sec: number }
  | { type: "one_shot";   at: string }                   // RFC3339
  | { type: "depends_on"; jobs: string[] };              // AND 시맨틱, on-success 기본

interface JobStatus {
  name: string;
  trigger: JobTrigger;
  on_overlap: "skip" | "queue" | "parallel";
  last_run?: JobRunSummary;
  next_run_at?: string;                  // RFC3339, cron/interval/one_shot 에서만
  success_rate_recent?: number;          // 최근 N 회 기준 0.0 ~ 1.0
  dependencies: { upstream: string[]; downstream: string[] };
}

interface JobRunSummary {
  run_id: string;
  state: JobRunState;
  ended_at?: string;
  duration_sec?: number;
}

interface JobRun {
  run_id: string;
  job_name: string;
  triggered_by:
    | { type: "schedule" }
    | { type: "manual" }
    | { type: "dependency"; upstream_run_id: string };
  scheduled_at: string;
  started_at?: string;
  ended_at?: string;
  exit_code?: number;
  state: JobRunState;
}

type JobRunState =
  | "pending"
  | "running"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "skipped";
```

---

## 5. 오류 응답

`ARCHITECTURE.md` §5.4를 정식 레퍼런스로 삼는다. 본 문서에서는 API별 대표 `code`를 참고용으로 열거.

| `code` | HTTP 상태 | 상황 |
|---|---|---|
| `invalid_request` | 400 | 요청 바디/쿼리 파라미터 형식 오류 |
| `invalid_config` | 400 | `ProcessConfig` 검증 실패 |
| `invalid_cron_expression` | 400 | `JobConfig.trigger.type = "cron"` 의 `expr` 문법 오류 |
| `process_not_found` | 404 | 해당 이름의 프로세스가 등록되지 않음 |
| `job_not_found` | 404 | 해당 이름의 Job 이 등록되지 않음 |
| `run_not_found` | 404 | 해당 Job 에 해당 `run_id` 의 Run 이 없음 |
| `name_conflict` | 409 | 같은 이름의 **프로세스** 가 이미 등록됨 (POST) |
| `job_name_conflict` | 409 | 같은 이름의 **Job** 이 이미 등록됨 |
| `already_running` | 409 | 실행 중이라 동작 거부 (Process) / `on_overlap = "skip"` 인 Job 의 수동 trigger 요청 거부 |
| `queued` | 409 | `on_overlap = "queue"` 상태에서 trigger 가 큐에 삽입됨 (응답 바디에 순번) |
| `not_running` | 409 | 실행 중이 아니라 동작 거부 |
| `crash_loop_detected` | 409 | 자동 재시작 중단 상태. 사용자 `restart` 호출로 해제 |
| `has_dependents` | 409 | Job 삭제 시 downstream 의존 존재. `?force=true` 로 의존 해제 후 삭제 |
| `run_already_finished` | 409 | 이미 종료된 Run 에 대한 cancel 시도 |
| `cycle_detected` | 422 | `JobConfig.trigger.type = "depends_on"` 이 순환을 형성 |
| `spawn_failed` | 500 | OS 레벨 spawn 실패 (권한·바이너리 부재 등) |
| `internal_error` | 500 | 그 외 데몬 내부 오류 |

오류 응답 바디 예시는 `ARCHITECTURE.md` §5.4 참조.

---

## 6. 버전 정책

- 본 API는 `/api/v1/` prefix 하에 **하위 호환 변경만** 허용한다 (필드 추가, 새 엔드포인트).
- 기존 필드 타입 변경·필드 제거·의미 변경은 **breaking change**로 간주하며 `/api/v2/`를 신설해 병행 서비스 후 이전한다.
- WebSocket 이벤트 `type` 문자열은 안정 키로 취급한다. 동일한 이벤트의 의미 변경 금지, 대신 새 `type` 도입.
- 데몬 릴리즈의 `GET /api/v1/daemon/status.version`은 SemVer(ROADMAP "버전 체계" 참조)를 반환한다. 클라이언트는 이 값으로 호환성을 판단할 수 있다.

---

## 7. 미확정 / PoC 확인 항목

다음은 본 문서 초안 시점에 확정되지 않은 항목이며 PoC·MVP 구현 시 확정한다.

- 페이지네이션: `GET /api/v1/processes`·`GET /api/v1/jobs` 가 많은 엔티티에서 페이징이 필요한지 (MVP 목표 1000 프로세스 기준)
- 대량 로그 조회의 응답 상한 정책 (현재 `tail <= 10000` 가정)
- WebSocket 이벤트의 시퀀스/id 부여 여부 (재연결 시 유실 판정용)
- `GET /api/v1/jobs/{name}/runs` 의 이력 보존 한도 (현재 Job 별 `log_retention` 설정에 위임; 서버측 글로벌 상한 필요 여부는 PoC·MVP 중 확정)
- 인증 도입 시점·형태 (현재는 로컬 전용, 원격 지원은 Post-Production)
