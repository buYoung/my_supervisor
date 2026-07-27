# macOS production schedule contract — Wave 2 v2 evidence

## 최초 출시 신규 Job 계약

- TOML bootstrap, HTTP direct/config apply, CLI config apply, Tauri/GUI 신규 Job 입력에서 timezone 생략은 현재 macOS IANA timezone, misfire policy 생략은 `run_once`, trigger ID 생략은 새 UUID를 사용한다.
- explicit timezone, misfire policy, trigger UUID는 그대로 보존한다.
- timezone 해석 실패는 UTC로 대체하지 않고 stable `invalid_config`/HTTP 400으로 전파한다.
- 저장소에 남아 있는 방어적 read fallback은 신규 입력 기본값이 아니다. 배포된 이전 revision의 migration이나 과거 기본값 보존은 최초 출시 gate가 아니다.

## v2 검증 기록

| 경계 | 관찰 | 상태 |
|---|---|---|
| config conversion | `omitted_job_fields_use_current_macos_defaults` exit `0`; runtime IANA zone, `run_once`, non-nil UUID exact assertion | pass |
| HTTP direct/config apply | omitted-field contract tests exit `0`; 두 경로가 공통 `job_config_to_job` 사용 | pass |
| CLI | 실제 daemon 조회에서 non-empty timezone, `run_once`, non-empty UUID | pass |
| GUI | HTTP/Tauri omitted Job response의 timezone/misfire가 공통 mapping을 거쳐 UI model/view까지 전달 | pass |
| invalid timezone | `SchedulerError::InvalidTimezone` → `AppError::InvalidConfig("invalid_timezone: ...")` | pass |

## 제한

DST transition, 전체 catch-up/retry/queue/dependency, 장기 downtime 및 admission 조합은 이번 대표 검증에서 실행하지 않았다. 이 광범위 schedule matrix는 `partial`이며 core process/CLI/GUI 최초 배포 gate의 pass로 확장 해석하지 않는다.
