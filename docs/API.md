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

---

## 3. WebSocket 엔드포인트

| 경로 | 설명 |
|---|---|
| `/api/v1/events` | 전역 이벤트 스트림 |
| `/api/v1/processes/{name}/logs` | 특정 프로세스 로그 follow |

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

### 3.2 /api/v1/processes/{name}/logs

접속 시 실시간 로그 라인을 스트리밍. 포맷은 REST `/logs`의 `lines` 요소와 동일.

- **Rate limit**: 초당 라인 상한 초과 시 `{ "type": "log.dropped", "payload": { "count": N } }` 제어 프레임을 삽입 (DD-012).
- 연결 종료: 클라이언트가 close, 또는 프로세스 등록 해제 시 서버가 close frame + `code` 전송.

### 3.3 공통 사항

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

`ARCHITECTURE.md` §14 설정 파일 예시의 `[[process]]` 블록과 대응.

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

하위 객체의 필드는 `ARCHITECTURE.md` §14 / §7 / §8 / §9 / §12와 일치한다.

---

## 5. 오류 응답

`ARCHITECTURE.md` §5.4를 정식 레퍼런스로 삼는다. 본 문서에서는 API별 대표 `code`를 참고용으로 열거.

| `code` | HTTP 상태 | 상황 |
|---|---|---|
| `invalid_request` | 400 | 요청 바디/쿼리 파라미터 형식 오류 |
| `invalid_config` | 400 | `ProcessConfig` 검증 실패 |
| `process_not_found` | 404 | 해당 이름의 프로세스가 등록되지 않음 |
| `name_conflict` | 409 | 같은 이름이 이미 등록됨 (POST) |
| `already_running` | 409 | 실행 중이라 동작 거부 |
| `not_running` | 409 | 실행 중이 아니라 동작 거부 |
| `crash_loop_detected` | 409 | 자동 재시작 중단 상태. 사용자 `restart` 호출로 해제 |
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

- 페이지네이션: `GET /api/v1/processes`가 많은 프로세스에서 페이징이 필요한지 (MVP 목표 1000 프로세스 기준)
- 대량 로그 조회의 응답 상한 정책 (현재 `tail <= 10000` 가정)
- WebSocket 이벤트의 시퀀스/id 부여 여부 (재연결 시 유실 판정용)
- 인증 도입 시점·형태 (현재는 로컬 전용, 원격 지원은 Post-Production)
