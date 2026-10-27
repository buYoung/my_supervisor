# 운영 상태와 작업 결과

`prerequisite_ready: true` — STATE-01~08 검증과 관련 검사를 완료했다.

## 기준과 변경

05 인계의 일정·시간대와 초안 저장 계약을 유지했다. 변경 전 실제 WebView 실행 tCmOod에서 헤더 새로고침이 작업 목록을 갱신하지 않고, 한 대상의 요청 중 다른 대상도 비활성화되며, 선택을 바꿔도 이전 대상의 로그가 남는 현상을 확인했다. 직접 invoke 오류 객체의 code도 소실됐다. [기준 기록](06-artifacts/baseline/).

공통 조회는 헤더와 기능 화면이 공유한다. 같은 조회의 중복 요청을 합치고 세대 번호로 늦은 응답을 배제한다. 로딩·등록 없음·검색 결과 없음·최초 조회 실패·마지막 성공 값·부분 결과를 구분한다. 목록은 100개씩 기존 cursor/watermark 계약으로 조회하며, 그룹 계산과 메타데이터 요청은 관련 데이터 변경에 한정한다.

상세 상태·설정·인스턴스·로그·실행 이력은 대상 ID를 키로 사용한다. 작업 진행과 오류는 대상별로 유지한다. 순차 재시작은 phase와 실제 outcome을 표시하고, 응답을 잃으면 같은 operation ID로 결과를 확인한다. 이벤트는 payload의 상태 전이로 설명한다. 등록 삭제는 실행 여부에 맞게 확인하며 기존 force/의존성 보호를 유지한다.

## 검증과 재현

저장소 루트 `/Users/buyonglee/Documents/work/private/my_supervisor`에서 `pnpm --dir crates/desktop/ui test:native --spec=state.spec.mjs`로 앱을 빌드했고, 테스트 조건 보정 후 `pnpm --dir crates/desktop/ui test:native --no-build --spec=state.spec.mjs`로 8개 전부 통과했다. 실제 실행된 내부 cwd/명령도 [manifest](06-artifacts/native/manifest.json)에 있다.

macOS 15.7.4, WebKit 605.1.15, debug/embedded Tauri, revision `748537710413f224be9f25a65d114426b33c0770`, 앱 소스 SHA-256 `542232c63112d57565d73029f175848df802dd3f18ce936363ed8431a661cda8`. 테스트 코드만 바뀐 재실행이므로 manifest의 테스트 소스 해시와 이미 빌드한 앱의 해시를 구분한다. [앱 식별](06-artifacts/native/identity.json), [개별 결과](06-artifacts/native/results.jsonl), [값과 소요 시간](06-artifacts/native/state-results.json).

| 사례 | 실제 확인 |
|---|---|
| STATE-01 | N-R: 실제 지연 응답의 로딩과 등록 없음, 검색 없음/초기화, 부분 목록, 임시 DB 조회 장애와 복구. 마지막 성공 행 유지 |
| STATE-02 | N-R: 헤더로 실제 목록 갱신. 지연된 이전 응답 뒤 GUI 신규 등록 결과 유지 |
| STATE-03 | N-R: A/B 로그와 실행 이력 전환 후 이전 대상 내용 배제. B 삭제 후 조회 실패에서도 A 내용으로 대체하지 않음 |
| STATE-04 | N-R: 실제 HTTP readiness 실패. 실패 1·미시도 1, failed_closed, 기존 정상 인스턴스 ID 유지 |
| STATE-05 | N-R: 실제 start/crash/stop 이벤트의 payload 전이와 crash 심각도. 알 수 없는/unsupported 값은 보조 단위 검사로 명시적 표시 확인 |
| STATE-06 | O + N-R: CUA가 실제 Return 전송. trusted keydown/click/keyup 기록, 정확히 한 번 시작, B 상세 선택 유지 |
| STATE-07 | N-R: B 요청 응답을 지연해도 A 중지 가능. 실제 완료 응답을 의도적으로 잃은 뒤 같은 ID 재조회 시 인스턴스 ID가 더 바뀌지 않음 |
| STATE-08 | N-R: 실제 250개 작업, 50개 실행 인스턴스, 3페이지와 검색/부분 경계. 등록 1832ms, 시나리오 17470ms. 성능 합격 임계값은 추가하지 않음 |

지연은 실제 invoke 결과의 전달만 늦춘다. STATE-07의 응답 소실은 클라이언트 래퍼가 실제 성공 직후 오류를 던진 통제 조건이다. 백엔드 실패로 표기하지 않는다. 부분 결과는 테스트 기능에서만 기존 PagePartitionFailureSource에 임시 root의 지정값을 연결한다. DB 장애도 해당 root의 jobs 테이블 이름만 변경하고 finally에서 복원했다.

[첫 실행](06-artifacts/first-native-attempt/)의 잘못된 CSS 선택자, 준비 검사 첫 주기 대기, 비활성 버튼 대기를 보정했다. [키보드 진단](06-artifacts/keyboard-driver-diagnostic/)에서 WebDriver Enter는 untrusted 이벤트만 보내 기본 버튼 동작을 실행하지 않는 것을 확인했다. 제품에 합성 키 특례를 넣지 않았다. 실제 CUA 입력의 [trusted 이벤트](06-artifacts/native/keyboard-observation.json)를 사용한다. 다음 전체 실행도 keyboard-ready.json 시점에 CUA Return을 보내야 한다.

## 검사·정리·인계

[검사 명령과 경고](06-artifacts/checks.json): UI typecheck/build, resource/event 단위 검사, 기존 parity/event 검사, workspace 및 desktop-e2e cargo check가 exit 0. Vite HMR의 기존 EPERM은 별도 환경 경고이며 네트워크 통합 성공으로 부르지 않는다. 삭제 확인 계약에 맞춰 기존 parity 검사도 취소·비강제 삭제를 확인하도록 보완했다.

[정리](06-artifacts/native/cleanup.json)에서 앱, 소유한 자식과 WebDriver/HTTP port 종료를 확인했다. 실제 사용자 설정·프로세스는 사용하지 않았다. public DTO/ID/expected revision/cancellation 계약을 유지했고, 테스트 fixture 제어는 desktop-e2e 기능에만 있다. 07은 이 조회·선택·저장 계약을 보존하며 흐름 편집을 이어간다.

07 저장 회귀에서 보조 그룹 갱신을 저장 완료와 함께 기다리는 결함을 발견했다. 그룹 갱신을 비동기로 분리했다. 동일 앱의 [저장 회귀 6개](06-artifacts/settings-regression/results.jsonl)가 통과했고 [정리](06-artifacts/settings-regression/cleanup.json)도 완료해 인계를 다시 확정했다. 지연 테스트는 동시에 발생하는 실제 조회를 같은 gate로 해제하도록 보정했다.
