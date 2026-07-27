# API Reference

**호스트**(데스크톱 `desktop` Tauri 앱 또는 헤드리스 `msv-daemon` launcher)가 제공하는 HTTP / WebSocket API 를 정리합니다. 두 경로 모두 `daemon` 런타임 조립을 공유하므로 API 는 동일합니다(DD-002). 클라이언트는 호스트의 WebView · 외부 브라우저 · `msv` CLI · 스크립트입니다. 내부적으로는 `crates/infra/http` adapter 가 `core::ports::HttpServer` 를 구현하여 이 엔드포인트를 호스팅합니다.

> **현재 문서 상태:** macOS 로컬 운영 MVP의 구현된 `/api/v1` route와 `crates/shared` DTO가 현재 계약입니다. Rules와 별도로 미지원이라고 표시된 필드는 설계 초안이며 실제 route/DTO 계약이 아닙니다.

관련 문서: [아키텍처](./ARCHITECTURE.md) · [설계 결정](./DESIGN_DECISIONS.md) · [로드맵](./ROADMAP.md)

---

## 1. 원칙

- **바인딩**: `127.0.0.1:<port>` (기본 9876). `0.0.0.0` 바인딩은 코드 레벨에서 금지 (DD-011).
- **인증**: 모든 `/api/v1` HTTP·WebSocket route는 `Authorization: Bearer <token>`을 먼저 검증한다. 토큰은 설치별 user-only credential file에서 native CLI/proxy가 읽으며, bearer를 WebView·URL·query string·로그·local storage·UI state에 노출해서는 안 된다. 자세한 현재/목표 transport 경계는 §8을 따른다.
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
| `POST` | `/api/v1/processes/{name}/restart` | 재시작 (crash loop 카운터 리셋). **`management_mode = SystemRegistered` 시 no-op + 안내 메시지** — OS 가 재시작 담당 (DD-025) |
| `GET` | `/api/v1/processes/{name}/logs` | 최근 로그. 쿼리: `tail`, `since`, `after_sequence`. WebSocket으로 연결하면 cursor 경계 뒤의 새 로그를 실시간 전달 |
| `POST` | `/api/v1/processes/{name}/convert` | 관리 모드 전환. body: `{ to: "direct" \| "system_registered", unit_name?: string, auto_start?: boolean }` |

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
- `management_mode = SystemRegistered` 인 경우 `200 OK` + `{ "noop": true, "reason": "managed_by_system" }` 응답. 실제 재시작은 OS `Restart=` 지시어에 위임 (§6.4, DD-025).

#### POST /api/v1/processes/{name}/convert

관리 모드를 Direct ↔ SystemRegistered 로 전환한다 (§6.4, DD-025).

요청:

```json
{
  "to": "system_registered",
  "unit_name": "my-supervisor-managed-api-server",   // to=system_registered 시 필수 (기본값: 자동 생성)
  "auto_start": true                                  // 전환 후 즉시 기동 여부, 기본 false
}
```

절차:
1. 현재 실행 중이면 현재 모드 경로로 stop
2. 현재 모드 흔적 정리 (System → `unregister`, unit 파일 삭제)
3. 새 모드로 등록 (Direct 면 단순 설정 변경, System 이면 `register`)
4. `ProcessSpec.management_mode` 업데이트·저장
5. `auto_start: true` 면 새 모드로 start

응답:
- `200 OK` — 전환 성공. 바디: `ProcessStatus`
- `404 process_not_found`
- `400 invalid_request` — `to` 값이 지원 범위 밖이거나 `unit_name` 포맷 오류
- `409 unit_name_conflict` — 지정된 `unit_name` 이 이미 다른 곳에 존재
- `500 service_registration_failed` — OS 서비스 매니저 등록 실패. 원래 모드로 자동 롤백된 뒤 반환

#### GET /api/v1/processes/{name}/logs

쿼리:

| 파라미터 | 기본값 | 설명 |
|---|---|---|
| `tail` | `100` | 반환할 최근 라인 수 (최대 10000) |
| `since` | *(생략 시 무제한)* | RFC3339 타임스탬프. 해당 시각 이후 로그만 |
| `after_sequence` | *(생략 시 제한 없음)* | 이 sequence보다 큰 로그만. 재연결 gap 복구에 사용 |

응답 예시:

```json
{
  "lines": [
    {
      "timestamp": "2026-04-21T10:30:45.123Z",
      "sequence": 42,
      "stream": "stdout",
      "line": "Server listening on :3000"
    }
  ],
  "truncated": false,
  "dropped_count": 0,
  "high_watermark": 42,
  "next_sequence": 43
}
```

- `truncated`: `tail`/`since` 필터로 잘려나간 라인이 있는지
- `dropped_count`: 백프레셔로 버려진 라인 수 (DD-012)
- `sequence`: source-local 단조 증가 cursor. `after_sequence`과 WebSocket 재연결에 사용한다.
- `high_watermark`: 이 snapshot을 만들 때 확정된 마지막 sequence이며, `next_sequence`은 다음 gap 조회 cursor다.

### 2.2 데몬 리소스

| 메서드 | 경로 | 설명 |
|---|---|---|
| `GET` | `/api/v1/daemon/status` | 데몬 상태 |
| `GET` | `/api/v1/daemon/recovery` | 보류 중인 durable 복구 진단 |
| `POST` | `/api/v1/daemon/reload` | 설정 파일 리로드 (SIGHUP과 동등) |
| `POST` | `/api/v1/daemon/config/validate` | 설정 batch 검증 및 diff 반환 |
| `POST` | `/api/v1/daemon/config/apply` | 설정 batch를 원자적으로 적용 |
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

#### GET /api/v1/daemon/recovery

보류 중인 recovery record를 최대 100개씩 종류별로 반환한다. record는 `kind`, `id`, `resource`, `stage`, `attempts`, 선택적 `last_error`만 포함하며 command, environment, PID, native identity는 노출하지 않는다. 완료된 record는 응답에 포함되지 않는다.

```json
{
  "records": [
    {
      "kind": "transient_cleanup",
      "id": "a2b8c4c8-9a9f-4cd3-ae6b-6d12af4e1ad0",
      "resource": "nightly-report/6a1cc0bb-0e6e-4c46-a4dc-67d5feae044f",
      "stage": "persist_terminal",
      "attempts": 2,
      "last_error": "database temporarily unavailable"
    }
  ]
}
```

`msv daemon status --recovery`는 같은 정보를 조회한다. 이 표면은 pending durable recovery의 관찰용이며, 완료 여부 또는 consumer별 event delivery를 보장하지 않는다.

#### POST /api/v1/daemon/reload

- `202 Accepted` — 리로드 시작.
- `400 invalid_config` — 리로드 대상 설정이 유효하지 않음. 데몬은 기존 설정 유지.
- 파일 리로드는 선언 파일을 권위로 하는 `replace` 적용이다. 명시적 `merge`는 `/daemon/config/apply` 요청에서만 선택한다.

#### POST /api/v1/daemon/config/validate · /apply

요청은 두 endpoint가 같은 형식을 사용한다.

```json
{
  "mode": "merge",
  "dry_run": false,
  "config": { "process": [], "job": [] }
}
```

- `mode`: `merge`는 지정 항목만 병합하고, `replace`는 누락된 process/job을 제거한다.
- `validate`는 항상 변경 없이 동일 diff를 반환한다. `apply`의 `dry_run: true`도 변경 없이 diff만 반환한다.
- 성공 응답은 `{ apply_id?, mode, diff, dry_run }`이며 `diff`는 process/job의 added/updated/removed 이름 배열을 담는다.
- scheduler/registrar 준비 단계에서 실패하면 apply는 이전 snapshot으로 보상한다. 다만 `replace` 제거의 Run 취소, 기존 Direct stop, DB commit 또는 새 Direct start 이후에는 이전 실행 집합을 복원했다고 주장하지 않는다. 이 경우 응답은 `409 config_recovery_required`이고 오류 본문에 durable `apply_id`가 포함되며, journal은 목표 snapshot으로의 forward recovery를 완료할 때까지 다음 apply를 거부한다. 복구의 Direct 실행 목표는 target의 `autostart=true` 항목뿐 아니라 적용 직전 실행 중이었고 target에도 남아 있는 Direct 항목을 포함한다. 따라서 변경된 `autostart=false` 항목도 target spec으로 다시 시작한다. 각 target spawn 의도와 확인된 native generation은 journal에 남으며, 재시작 뒤 동일 identity가 확인되면 중복 spawn하지 않는다.

#### POST /api/v1/daemon/shutdown

- `202 Accepted`. 새 dispatch를 닫은 뒤 queued Run을 취소하고 active Run의 child/pump 완료를 기다린 다음 tied 자식을 회수한다. 회수 실패는 성공 종료로 축약하지 않는다.

### 2.3 Jobs 리소스

`ARCHITECTURE.md §12` (Jobs 배치 스케줄러) 의 wire 인터페이스. Job 등록·수정·삭제 + 수동 트리거 + Run 이력 조회가 한 set.

| 메서드 | 경로 | 설명 |
|---|---|---|
| `GET` | `/api/v1/jobs` | 등록된 Job 목록 |
| `POST` | `/api/v1/jobs` | Job 등록 (body: `JobConfig`) |
| `GET` | `/api/v1/jobs/{name}` | Job 상세 |
| `PATCH` | `/api/v1/jobs/{name}` | Job 수정 (부분 업데이트) |
| `DELETE` | `/api/v1/jobs/{name}` | Job 제거 (`?force=true`은 해당 Job의 own Run 취소·회수만 허용) |
| `POST` | `/api/v1/jobs/{name}/trigger` | 수동 즉시 실행 (trigger 타입과 무관) |
| `GET` | `/api/v1/jobs/{name}/runs` | Run 이력. 쿼리: `limit`, `since`, `state` |
| `GET` | `/api/v1/jobs/{name}/runs/{run_id}` | Run 상세 |
| `POST` | `/api/v1/jobs/{name}/runs/{run_id}/cancel` | 진행 중 Run 중단 |
| `GET` | `/api/v1/jobs/{name}/runs/{run_id}/logs` | Run 로그 (REST/WS). 쿼리: `tail`, `since`, `after_sequence` |

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

- `204 No Content` — scheduler 등록, active/queued/cleanup Run, Job/Run 행과 해당 sealed JSONL 로그, `run_log_cleanup` 및 deletion journal까지 모두 제거된 뒤에만 반환한다. Job/Run 행 삭제, 로그 정리 대기열 등록, 삭제 저널의 `rows_deleted` 전이는 하나의 durable commit으로 기록된다.
- `404 job_not_found`
- `409 has_dependents` — downstream 의존 Job 존재. `force`는 의존 그래프를 변경하지 않는다.
- `409 job_has_active_runs` — `force=false`에서 대기/실행 Run이 남아 있음.
- `409 job_deletion_recovery_required` — `force=true` 삭제가 취소 이후의 비가역 단계에서 일시 실패했다. 같은 요청을 재시도하거나 데몬 재시작 뒤의 복구가 같은 deletion id로 삭제를 계속한다. 취소 이전의 scheduler/queued Run 저장 실패는 `rollback_required`로 기록하고 기존 Job의 dispatch와 scheduler 등록, journal 제거만 재시도하며 삭제 경로로 다시 진입하지 않는다.

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
| `state` | *(생략 시 전부)* | `pending` / `running` / `succeeded` / `failed` / `timed_out` / `cancelled` / `skipped` 중 하나 |

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

### 2.4 Rules 리소스 (자동화)

> **미지원·설계 초안:** 현재 구현에는 Rules domain/DTO/route가 없습니다. 아래 인터페이스는 향후 설계이며 `/api/v1/rules`를 호출할 수 있다는 의미가 아닙니다.

`ARCHITECTURE.md §13` (Rules) 의 wire 인터페이스. 이벤트→액션 자동화의 CRUD + 수동 발화 + 발화 이력 + macOS 권한 조회.

| Method | Path | 설명 |
|---|---|---|
| `GET` | `/api/v1/rules` | 등록된 Rule 목록 |
| `POST` | `/api/v1/rules` | Rule 등록 (body: `RuleConfig`). 윈도우·핫키 트리거/액션 포함 시 비-macOS 호스트는 `422 not_supported_on_platform` (DD-027) |
| `GET` | `/api/v1/rules/{name}` | Rule 상세 |
| `PATCH` | `/api/v1/rules/{name}` | Rule 수정 (부분 업데이트, `enabled` 토글 포함) |
| `DELETE` | `/api/v1/rules/{name}` | Rule 제거 |
| `POST` | `/api/v1/rules/{name}/fire` | 수동 시험 발화 (trigger 무관 즉시 액션 실행) |
| `GET` | `/api/v1/rules/{name}/fires` | 발화 이력. 쿼리: `limit`, `since`, `state` |
| `GET` | `/api/v1/automation/permissions` | macOS 자동화 권한 상태 (Accessibility / Input Monitoring). 비-macOS 호스트는 `supported: false` |

**플랫폼 거부**: 윈도우·핫키·macOS 시스템 이벤트를 쓰는 Rule 을 비-macOS 호스트(헤드리스 데몬 포함)에 등록하면 `422 not_supported_on_platform` 을 반환한다 (DD-027). 권한 미부여 상태로 등록하면 수락하되 `enabled = false (permission_required)` 로 표기한다 (DD-029).

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
  "event_id": "c0f10fc5-7f64-4dc5-9e42-69a9b5161c89",
  "timestamp": "2026-04-21T10:30:45.123Z",
  "payload": { ... }
}
```

`event_id`는 additive 안정 UUID다. 구 daemon은 이 필드를 생략할 수 있고, 수신자는 ID 없는 envelope을 정상 수신해야 한다. 새 daemon은 `job.run_succeeded`, `job.run_failed`, `job.run_timed_out`, `job.run_cancelled`의 terminal frame과 모든 `process.state_changed` frame에 ID를 보낸다. process 상태 이벤트의 ID는 관찰 frame 식별자일 뿐 durable outbox/receipt 보장을 뜻하지 않는다.

terminal 이벤트는 SQLite outbox에서 연결된 외부 transport 중 하나가 실제 write에 성공할 때까지 재시도하는 at-least-once 전달이다. 연결 전 전체 history나 소비자별 exactly-once를 제공하지 않으며, write 성공 뒤 acknowledgement 전 daemon crash가 나면 같은 `event_id`가 재전송될 수 있다. CLI와 desktop renderer는 세션 메모리의 bounded ID cache로 이를 중복 제거한다. 이 cache는 재시작 뒤에는 비어 있으므로 영구 exactly-once를 주장하지 않는다.

이벤트 타입:

| `type` | `payload` 요약 |
|---|---|
| `process.state_changed` | `{ name, from, to, definition_id, instance_id, generation }` — `ProcessState` 전이. `instance_id`와 `generation`은 저장소가 슬롯 조회를 지원하지 않으면 `null`일 수 있다. |
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

접속 시 서버는 먼저 cursor snapshot을 보내고, 그 snapshot의 `high_watermark`보다 큰 실시간 로그만 이어서 전송한다. 포맷은 REST `/logs`의 `lines` 요소와 동일이다.

- **Rate limit**: 초당 라인 상한 초과 시 `{ "type": "log.dropped", "payload": { "count": N, "after_sequence": S } }` 제어 프레임을 삽입한다. 클라이언트는 `after_sequence=S`로 REST gap을 채운 뒤 재구독한다 (DD-012).
- 연결 종료: 클라이언트가 close, 또는 프로세스 등록 해제 시 서버가 close frame + `code` 전송.

### 3.3 /api/v1/jobs/{name}/runs/{run_id}/logs

해당 Run 의 stdout/stderr 라인을 실시간 스트리밍한다. REST와 WebSocket 모두 `tail`, `since`, `after_sequence`를 journal 조회 전에 적용하므로, Run도 process와 동일하게 디스크 backfill 뒤 limit을 적용한다. 포맷·rate limit·drop 규칙은 §3.2 와 동일하다. Run 종료 시 서버가 close frame 으로 종료한다(정상·실패 여부는 `/events` 의 `job.run_*` 메시지 참조). CLI `--follow`는 REST backfill, WebSocket 연결·읽기, 재시도 대기 중 일시적 장애를 제한된 지수 backoff로 재시도하며 Ctrl-C에는 정상 종료한다.

구 legacy 로그는 source sequence/timestamp interleave를 정확히 복원할 수 없다. legacy 파일은 `since`와 cursor 정확성 보장 대상이 아니며, 새 journal 구간에서만 무유실·무중복 재연결을 보장한다. 특히 기존 detached `direct-<sanitized>.stdout.log`/`stderr.log` 쌍은 등록된 전체 이름에서 충돌하지 않을 때에만 cursor 없는 일회성 tail로 읽는다. 충돌한 이름은 어느 process에도 노출하지 않으며, 새 detached spawn은 `msv-log-proxy`가 한 `direct-<hex>.jsonl` journal에 stdout/stderr 순서와 sequence를 함께 기록한다.

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
  management_mode: ManagementMode;
  pid: number | null;              // 실행 중이면 Direct/SystemRegistered 모두 값 존재
  unit_name: string | null;        // SystemRegistered 모드에서만 값 존재
  restart_count: number;
  started_at: string | null;       // RFC3339
  cpu_percent: number;
  memory_bytes: number;
}

type ManagementMode =
  | { type: "direct" }
  | { type: "system_registered"; unit_name: string };
```

U03 추가 응답 계약: 새 daemon은 `ProcessStatus`에 `definition_id`(UUID), `desired_instances`(기본 `1`), `instances`를 추가한다. `instances`의 각 항목은 `instance_id`(UUID), `ordinal`, `generation`, `state`, `pid`, `restart_count`, `started_at`(RFC3339 또는 `null`), `cpu_percent`, `memory_bytes`를 포함한다. 이전 daemon 응답에는 이 additive 필드가 없을 수 있으며, 수신자는 이를 각각 ID 없음, `desired_instances=1`, 빈 목록으로 해석한다.

U08 추가 응답 계약: 새 daemon은 선택적 `guard` 객체를 추가한다. `process_id`, `native_generation`, `observed_at`, `liveness`, `readiness`, `memory`, `watch`, `last_restart_cause`, `last_error`, `is_historical`을 포함한다. 상태 값은 `unknown`, `healthy`, `unhealthy`, `unsupported`이며, restart cause는 `watch_changed`, `memory_ceiling`, `liveness_failure`이다. `is_historical=true`인 persisted guard snapshot은 이전 daemon의 증거일 뿐 현재 generation의 readiness로 해석하면 안 된다. 새 daemon은 변경 시 `process.guard_changed` WebSocket 이벤트로 같은 `guard` 객체를 전달한다.

`instances`는 저장소가 지원하는 활성 슬롯을 ordinal 오름차순으로 제공한다. 현재 runtime 관찰은 ordinal `0`에만 연결하며, 아직 조정(reconciliation)되지 않은 다른 슬롯은 `stopped`, PID 없음, 사용량 0으로 표시한다. `desired_instances > 1`이면 legacy aggregate의 `pid`는 `null`, `started_at`은 관찰된 인스턴스의 가장 이른 시작 시각, `restart_count`·`cpu_percent`·`memory_bytes`는 인스턴스 합계다. aggregate state는 `running`, `starting`, `stopping`, `crashed`, `stopped` 순으로 관찰된 상태를 선택한다. 단일 인스턴스의 기존 aggregate 의미는 유지한다.

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

`ARCHITECTURE.md` §16 설정 파일 예시의 `[[process]]` 블록과 대응.

```ts
interface ProcessConfig {
  name: string;
  command: string;
  args?: string[];
  cwd?: string;
  env?: Record<string, string>;
  management_mode?: ManagementMode;   // 기본 { type: "direct" }
  lifecycle?: "tied" | "detached";    // 기본 "tied", Direct 모드에서만 의미
  autostart?: boolean;
  restart?: RestartPolicy;             // SystemRegistered 시 OS unit 의 Restart= 로 변환
  shutdown?: ShutdownPolicy;
  health_check?: HealthCheck;         // 미지원·설계 초안
  logging?: LoggingPolicy;            // 미지원·설계 초안
}
```

하위 객체의 필드는 `ARCHITECTURE.md` §16 / §7 / §8 / §9 / §14 과 일치한다. `management_mode` 의 시맨틱은 §6.4.

U03 추가 설정 계약: `definition_id`는 선택 UUID이며 생략하면 변환 경계가 이름 기반의 안정 ID를 만든다. `instances`는 선택 정수(생략 시 `1`)이고, `watch`, `memory`, `liveness`, `readiness`, `rolling`은 선택 정책 객체다. 시간은 모든 공개 설정 경계에서 `_ms` 단위, 메모리는 `ceiling_bytes`/`memory_bytes` 바이트 단위를 사용한다. 이 정책은 Direct 전용이며 SystemRegistered는 `instances=1`과 모든 Direct 전용 정책 비활성만 허용한다. 유효하지 않은 인스턴스·정책·모드 조합은 `invalid_config` 또는 요청 경계의 `invalid_request`로 어떠한 process·SQLite row·registrar 변경보다 먼저 거부된다.

### 4.4 JobConfig · JobStatus · JobRun

`ARCHITECTURE.md` §12 Jobs 섹션 및 §16 설정 예시의 `[[job]]` 블록과 대응.

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
  | "timed_out"
  | "cancelled"
  | "skipped";
```

`log_retention.max_runs`와 `max_age_days`는 1 이상의 값만 허용한다. 둘 다 설정하면 둘 중 하나라도 한도를 넘은 완료 기록과 해당 로그 파일을 삭제한다. 정리는 실행 종료 직후, 관리 프로그램 시작 시, 실행 중 매시간 수행한다. 실행 중이거나 대기 중인 기록은 삭제하지 않는다.

---

### 4.5 RuleConfig · RuleStatus · RuleFire

> **미지원·설계 초안:** 현재 `crates/shared`에는 아래 Rule wire type이 없습니다.

`ARCHITECTURE.md §13` (Rules) 및 §16 설정 예시의 `[[rule]]` 블록과 대응.

- **RuleConfig** (등록/수정 body): `name`, `trigger` (one-of: `file_change` | `system_event` | `hotkey`), `actions[]` (각 `start_process` | `stop_process` | `trigger_job` | `run_command` | `window`), `enabled`
- **RuleStatus** (조회 응답): `name`, `trigger` 요약, `actions` 요약, `enabled`, `last_fire` (시각·결과), `fire_count_recent`, `permission` (`granted` | `required` | `not_applicable`)
- **RuleFire** (발화 이력): `fire_id`, `rule_name`, `fired_at`, `triggered_by` (`event` | `manual`), `state` (`fired` | `actions_succeeded` | `actions_failed` | `skipped`), `action_results[]`

트리거·액션의 플랫폼 가용성은 호스트에 따라 다르다 (§2.4, DD-027). request/response 세부 필드는 구현 시 확정.

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
| `has_dependents` | 409 | Job 삭제 시 downstream 의존 존재. `force`는 의존 그래프를 변경하지 않음 |
| `job_has_active_runs` | 409 | `force=false` Job 삭제 시 대기 또는 실행 Run 존재 |
| `job_deletion_recovery_required` | 409 | force Job 삭제가 비가역 취소 경계 뒤에 있어 durable deletion journal로 forward recovery 중 |
| `run_already_finished` | 409 | 이미 종료된 Run 에 대한 cancel 시도 |
| `cycle_detected` | 422 | `JobConfig.trigger.type = "depends_on"` 이 순환을 형성 |
| `unit_name_conflict` | 409 | Direct → SystemRegistered 전환 시 지정된 `unit_name` 이 다른 곳에 이미 존재 |
| `spawn_failed` | 500 | OS 레벨 spawn 실패 (권한·바이너리 부재 등) |
| `service_registration_failed` | 500 | OS 서비스 매니저 등록·해제 실패 (권한·시스템 상태 문제). 원래 모드로 롤백된 뒤 반환 (DD-025) |
| `internal_error` | 500 | 그 외 데몬 내부 오류 |

오류 응답 바디 예시는 `ARCHITECTURE.md` §5.4 참조.

---

## 6. 버전 정책

### Schedule preview

`POST /api/v1/jobs/preview`는 `{ config, at, count }`를 받고 최대 100개의 UTC 예정 시각과 IANA local-time 표기를 반환한다. 이 호출은 job/run 행, scheduler timer 또는 event outbox를 생성·갱신하지 않는다. `count` 기본값은 10이고 탐색은 5년 또는 100,000 candidate에서 bounded error로 종료한다.

Job schedule의 additive 필드는 `timezone`, `schedule_revision`, `trigger_id`, `misfire_policy`, `retry_policy`, `admission`이다. 최초 출시의 새 Job은 TOML file bootstrap, HTTP `POST/PATCH /jobs`, HTTP config apply(및 이를 호출하는 CLI), Tauri/GUI 어느 입력 경계에서든 `timezone` 생략 시 현재 macOS IANA zone을 저장하고, `misfire_policy` 생략 시 `run_once`, `trigger_id` 생략 시 새 UUID를 사용한다. 시간대 해석 실패는 UTC로 조용히 대체하지 않는다. 명시한 timezone·misfire policy·trigger UUID는 그대로 보존한다. DST의 존재하지 않는 local time은 생략하고 반복 local time은 이른 UTC instant 한 번만 쓴다.

Schedule run은 `original_scheduled_at` 및 occurrence `(trigger_id, schedule_revision, scheduled_at, attempt)`를 additive로 노출한다. 기존/manual/dependency run의 이 필드는 null/omitted일 수 있다.

- 본 API는 `/api/v1/` prefix 하에 **하위 호환 변경만** 허용한다 (필드 추가, 새 엔드포인트).
- 기존 필드 타입 변경·필드 제거·의미 변경은 **breaking change**로 간주하며 `/api/v2/`를 신설해 병행 서비스 후 이전한다.
- WebSocket 이벤트 `type` 문자열은 안정 키로 취급한다. 동일한 이벤트의 의미 변경 금지, 대신 새 `type` 도입.
- 데몬 릴리즈의 `GET /api/v1/daemon/status.version`은 SemVer(ROADMAP "버전 체계" 참조)를 반환한다. 클라이언트는 이 값으로 호환성을 판단할 수 있다.

---

## 7. 미확정 / PoC 확인 항목

다음은 본 문서 초안 시점에 확정되지 않은 항목이며 PoC·MVP 구현 시 확정한다.

- 페이지네이션: 현재 `GET /api/v1/processes`·`GET /api/v1/jobs`는 cursor pagination이 없다. §8의 default `50`/cap `200`, opaque cursor, aggregate-query 계약은 U21 additive 구현 항목이다.
- 대량 로그 조회의 응답 상한 정책 (현재 `tail <= 10000` 가정)
- 모든 Job에 강제로 적용할 전역 이력 보존 상한 도입 여부. 현재는 Job별 `log_retention` 설정을 적용한다.
- native session bootstrap/cookie/CSRF/Origin 및 installed desktop proxy는 §8에서 계약을 고정했지만 아직 U21 구현·관찰 대상이다.

---

## 8. Wave 9 Operator capability 계약 고정

이 절은 Wave 9 U20의 권위 capability matrix다. `구현됨`은 아래에 든 현재 source의 동작만 뜻하며 installed desktop proxy, browser-free session, 새 aggregate API 또는 UI parity를 뜻하지 않는다. `U21 필수`는 shape를 지금 고정하되 source 구현은 다음 배치가 소유한다. 모든 새 wire DTO는 `crates/shared/src/api.rs`의 snake_case이며, 기존 `/api/v1`, Tauri command/event, `--output json`의 이름·shape·의미를 바꾸지 않는다.

공통 오류는 §5 `ErrorBody`이고, mutation의 부분 결과는 성공으로 축약하지 않는다. `ProcessOperationDto.outcomes`와 미래 aggregate response의 `partial`은 실패 partition 및 prior data 보존을 명시해야 한다. 현재 list의 한도는 개별 route가 실제로 검증하는 값이 권위이며, 아래 `50/200`은 기존 route를 재해석하지 않는 U21 additive 목표다.

| capability | HTTP 현재 계약 | Tauri / native 현재 계약 | CLI 현재 계약 | DTO·오류·cursor | credential/session 및 상태 |
|---|---|---|---|---|---|
| process definition/group | `GET/POST /processes`, `GET/DELETE /processes/{name}`, start/stop/restart/convert | `cmd_list_processes`, `cmd_get_process`, `cmd_add_process`, `cmd_start_process`, `cmd_stop_process`, `cmd_restart_process`, `cmd_remove_process`, `cmd_convert_process`는 embedded facade | `ps`, `show`, `add`, `start`, `stop`, `restart`, `remove`, `convert`; `--output json` | `ProcessConfigDto`, `ProcessListDto`, `ProcessStatusDto`; §5 stable errors; 현재 목록 cursor 없음 | HTTP/CLI bearer 구현됨. Tauri direct facade와 TS HTTP 무인증 path는 installed proxy가 아니므로 **U21 필수** |
| instance / scale / rollout | `GET /processes/{name}/instances`, `POST .../scale`, `POST .../rolling-restart` | `process_instances`, `scale_process`, `rolling_restart_process` embedded command은 있으나 UI client에 없음 | `instances`, `scale`, `restart --rolling`; partial outcome은 현재 CLI exit 3 | `ProcessInstancesDto`, `ProcessOperationDto`; Idempotency-Key/`operation_id`; list cursor 없음 | native proxy 및 OperationsClient parity, partial renderer는 **U21 필수** |
| process logs | `GET /processes/{name}/logs` REST/WS upgrade; `tail`, `since`, `after_sequence` | `cmd_process_logs`, `cmd_follow_logs`; tail/follow만 | `logs [--follow] [--tail] [--since]`; follow는 JSONL | `LogsResponseDto`, numeric `high_watermark`/`next_sequence`; retained cursor expiry를 명시 | bearer WS는 구현됨. cookie session + cursor-resume proxy는 **U21 필수**; 기존 CLI follow row-loss 제한은 별도 미해결 |
| job definition / preview | `GET/POST /jobs`, `GET/PATCH/DELETE /jobs/{name}`, `POST /jobs/preview` | `cmd_list_jobs`, `cmd_add_job`, `cmd_preview_job`, `cmd_remove_job`; update/get은 없음 | `job ls`, `show`, `remove`, `preview`; `--output json` | `JobConfigDto`, `JobStatusDto`, `JobPreviewRequestDto/JobPreviewDto`; current list cursor 없음 | HTTP/CLI bearer 구현됨; full invoke/TS parity·bounded form은 **U21/U22 consumer** |
| occurrence / attempt / cancel | `POST /jobs/{name}/trigger`, `GET .../runs`, `GET .../runs/{run_id}`, `POST .../cancel` | `cmd_trigger_job`, `cmd_list_runs`만; get/cancel 없음 | `job trigger`, `runs`, `cancel`; `job logs` | `JobRunListDto`, run DTO; current `limit` is route-owned, cursor 없음; §5 `run_*` errors | installed proxy and no-silent-partial aggregate run history are **U21 필수** |
| job logs | `GET /jobs/{name}/runs/{run_id}/logs` REST/WS upgrade | no invoke logs command | `job logs [--follow] [--tail] [--since]` | `LogsResponseDto`, numeric cursor/high-watermark | bearer WS 구현됨; native session and invoke parity는 **U21 필수** |
| metrics / events | `GET /observability/metrics`, `/events`; live `GET /events` WS | `cmd_list_metric_samples`, `cmd_list_operator_events`; global event forwarder | `observability metrics`, `events`; `daemon events` follow | `ObservabilityPageDto<MetricSampleDto/OperatorEventDto>`; opaque cursor is current for observability | HTTP/CLI bearer 구현됨; proxy/session and UI filters are **U21 필수** |
| alerts / acknowledgement / delivery | rules CRUD, `GET /observability/alerts`, `POST .../ack`, `GET .../deliveries` | list rules/episodes/deliveries, upsert, acknowledge embedded commands | `observability alerts`, `deliveries`; no rule mutation command | `AlertRuleDto`, `AlertEpisodeDto`, `DeliveryAttemptDto`, `ObservabilityPageDto`; cursor on paged records | bearer 구현됨; full CLI/invoke/TS parity and explicit partial UX are **U21 필수** |
| daemon / service maintenance | daemon status/recovery/reload/config/shutdown; authenticated service rotate-token/backup/upgrade/rollback | only `cmd_daemon_status` embedded | `daemon status/events/shutdown`, config commands; service lifecycle local and maintenance RPC | `DaemonStatusDto`, `RecoveryDiagnosticsDto`, config/service DTO; §5 errors | CLI native credential refresh 구현됨. Tauri installed native proxy and session revocation are **U21 필수** |
| config | `POST /daemon/config/validate`, `/apply` | no invoke command | `config validate|apply`; legacy `--output json` | `ConfigApplyRequestDto/ResultDto`; recovery-required stays error, not partial success | HTTP/CLI bearer 구현됨; invoke/UI parity는 **U21 필수** |

### Native session 및 transport 경계

- production Tauri와 CLI는 user-only credential을 native layer에서만 읽어 installed daemon에 주입한다. renderer는 bearer를 반환받거나 저장·조합하지 않는다. 현재 CLI만 이 경계를 구현했으며, 현재 desktop은 embedded facade와 debug-only devBridge이므로 production installed proxy로 표현하면 안 된다.
- U21 debug devBridge bootstrap은 one-time native command로 daemon-issued opaque session cookie와 memory-only non-secret CSRF nonce를 교환한다. cookie는 정확히 `10m`, `HttpOnly`, `SameSite=Strict`, exact-loopback `Path`, no `Domain`이며 bearer 자체가 아니다. mutation은 nonce를 보내고 WS upgrade는 cookie 및 strict Origin을 요구한다.
- daemon restart, bearer rotation, logout 또는 `15m` absolute age는 session을 폐기한다. native bootstrap은 재인증 후 마지막 numeric log cursor에서 재개한다. token rotation generation으로 기존 bearer/WS를 무효화하는 현재 HTTP primitive는 이 목표와 충돌하지 않는다.

### Pagination·aggregate·fixture 계약

- U21 additive list/aggregate endpoint는 기본 `50`, cap `200`, stable opaque cursor와 snapshot high-watermark를 사용한다. panel refresh는 resource family당 aggregate request 하나이며 client concurrency는 최대 `4`다. partition failure는 prior data를 보존하고 `partial`과 실패 partition을 표시한다. 현재 process/job list와 기존 UI fan-out은 이 계약을 아직 구현하지 않았다.
- U22 scale acceptance input은 `250 jobs`, `50 instances`, `10,000 entries`다. U20은 fixture나 test case를 만들지 않는다.
- U21 debug-only fixture control은 기존 debug daemon host에만 둔다. partition-failure/scale control 이름과 input은 `fixture=partition_failure|scale`, `resource_family`, `partition`, `enabled`로 고정하고, `enabled=false`는 injected fault를 제거한다. isolated `mktemp -d` root, unused loopback port, `MSV_DAEMON_TEST_DATA_DIR`, `MSV_DAEMON_TEST_CONFIG_PATH`, `MSV_DAEMON_TEST_BIND_ADDR`, `MSV_DAEMON_TEST_CONTROL_SOCKET`을 사용하며 public API/CLI로 setup·cleanup한다. cleanup target은 검증된 fixture root와 spawned process group뿐이다.

### CLI json-v2 및 exit 계약

기존 `--output json`은 opt-in `json-v2`로 대체하지 않는다. 현재 `print_json`의 pretty JSON document, log/event follow의 one-object-per-line JSONL, stdout success / stderr `error: ...`, 그리고 현 `CliError` exit (`0` success, `1` general/domain, `2` not found, `3` daemon unreachable 또는 current partial)는 byte-level compatibility 기준이다.

U21은 additive `--output json-v2`만 추가하며 모든 command에 versioned envelope `{"ok": boolean, "data": ..., "error": ..., "partial": ...}`를 쓴다. `error`와 `partial`은 각각 `null` 가능 필드이고 successful partial도 `partial`을 생략해 숨기지 않는다. json-v2 exit는 `0=success`, `1=domain failure`, `2=usage/validation`, `3=partial`, `4=daemon/auth/transport unavailable`으로 고정한다. 이것은 현 CLI exit를 소급 변경하지 않는다.

### U21 additive session transport surface

`POST /api/v1/session/bootstrap`은 native bearer로만 호출할 수 있다. 응답은 memory-only CSRF nonce와 만료 시각을 반환하고, opaque session id는 `HttpOnly; SameSite=Strict; Max-Age=600; Path=/api/v1` cookie로만 설정한다. `Domain` 속성은 설정하지 않는다. `POST /api/v1/session/logout`은 현재 cookie를 폐기하고 만료 cookie를 반환한다.

세션 인증은 loopback `Origin`을 요구하고 mutation에는 `X-CSRF-Token`이 필요하다. 세션은 최근 사용 10분, 절대 15분, bearer generation rotation, 또는 host restart에서 무효화된다. 기존 bearer route, legacy list DTO/route 및 WebSocket numeric cursor 의미는 변경하지 않는다.
