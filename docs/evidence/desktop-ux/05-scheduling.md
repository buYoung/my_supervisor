# 일정 편집과 실행 의미

`prerequisite_ready: true` — SCHEDULE-01~06과 입력·저장 회귀를 완료했다.

## 기준과 수정

[기준 실행](05-artifacts/baseline/schedule-baseline.json)에서 같은 호스트의 서버 기본값은 Asia/Seoul이었지만 GUI는 UTC를 저장했다. 미지정 timeout은 3600으로 표시됐고, cron 변경 뒤 이전 미리보기가 남았다.

GUI에서 임의의 UTC 및 숫자 제한 기본값을 제거했다. 일정 시간대는 표시 시간대와 분리하고, 생략한 값은 그대로 서버가 결정한다. 전체 제한 시간은 서버 기본/직접 지정으로 구분하며 미지정을 무제한으로 표시하지 않는다. 현재 전체 흐름의 기본 상한 86400초는 `workflow_engine.rs`의 실제 소비 경로에서 확인했다.

미리보기는 현재 입력과 요청 번호에 연결된다. 일정 변경 시 이전 날짜를 숨기고, 늦은 이전 응답은 반영하지 않는다. 일반 일정 예시에서 직접 입력으로 되돌리면 이전 cron을 복원한다. 한 번 실행하는 시각은 표시 시간대 변경 시 같은 순간으로 변환하며, 존재하지 않는 DST 입력과 과거 시각은 원문과 오류를 유지한다.

추가로 스케줄러 preview가 cron 파싱 오류를 빈 목록으로 삼키는 원인을 수정했다. 등록 때 사용하는 동일한 파서 오류를 preview에서도 반환하며, 기존 `invalid_cron_expression` 계약을 유지한다. 유효한 의존성 일정의 빈 결과는 계속 허용한다.

## 네이티브 증거

저장소 루트(`/Users/buyonglee/Documents/work/private/my_supervisor`)에서 `pnpm --dir crates/desktop/ui test:native --spec=schedule.spec.mjs`를 실행했다. macOS 15.7.4 debug/embedded Tauri, WebKit 605.1.15, revision `748537710413f224be9f25a65d114426b33c0770`, 빌드 소스 SHA-256 `eb4638127d73d5d4615b5ecd6c7cc5425789ce2d02b8e6bb990ddefad3e34ac9`. [앱 정보](05-artifacts/native/identity.json), [명령·fixture](05-artifacts/native/manifest.json), [개별 결과](05-artifacts/native/results.jsonl), [값 기록](05-artifacts/native/schedule-results.json)을 보존했다.

| 사례 | 실제 확인 |
|---|---|
| SCHEDULE-01 | N-R: GUI 생략 → Asia/Seoul, 인증된 HTTP 생성 → invoke 조회도 Asia/Seoul. explicit UTC/LA를 GUI 저장 후 보존 |
| SCHEDULE-02 | N-R: 미지정 timeout 저장·재열기 및 실제 실행 성공. explicit 1초가 저장되고 실제 자식이 약 1317ms 후 timed_out으로 종료 |
| SCHEDULE-03 | N-R: A 응답을 지연시키고 B 결과를 받은 뒤 A를 반환해도 B 유지. 잘못된 cron은 실제 백엔드 오류 표시 |
| SCHEDULE-04 | N-R: LA의 존재하지 않는 2027-03-14 02:30을 보존·차단. 표시 시간대를 바꿔도 저장 순간 2027-11-09T12:00:00Z 유지 |
| SCHEDULE-05 | N-R: 예시/직접 입력 복원, 평일 오전 9시 cron의 GUI 미리보기와 저장 후 HTTP 미리보기 일치 |
| SCHEDULE-06 | N-R: 명령만 바꾸면 trigger UUID 유지. 시간 조건을 바꾸면 기존 서버 계약대로 revision 증가·새 UUID 발급. timezone/misfire와 양 전송 조회 일치 |

트리거 ID가 시간 조건 변경에도 유지돼야 한다는 초기 테스트 가정은 현재 서버 계약과 달라 수정했다. 프로덕션 ID 생성 규칙을 바꾸지 않았다. [첫 실행 기록](05-artifacts/first-native-attempt/)을 남겼다.

24시간을 실제로 기다리는 검증은 하지 않았다. 미지정의 기본 한도는 실행기 코드에서 확인하고, 미지정 실행과 명시적 1초 종료를 실제로 검증했다. 이를 24시간 연속 실행 증거로 확대하지 않는다.

## HTTP 테스트 경로와 정리

실제 HTTP/invoke 비교를 위해 `desktop-e2e` 기능의 검증된 임시 root에만 인증된 HTTP 경로를 추가했다. 같은 facade/router를 사용하고 OS가 할당한 고유 loopback port를 `output/native-http.json`에 기록한다. 토큰은 해당 fixture root에서만 읽으며 로그·증거에 복사하지 않았다. 무인증 요청의 401을 확인했다. 일반 빌드의 리스너, CSP, 인증 정책과 release gate는 바꾸지 않았다.

[정리 결과](05-artifacts/native/cleanup.json): WebDriver와 HTTP port 종료, 앱 종료, timeout 자식 종료가 확인됐다. 정상/실패 실행 모두 사용자 상태를 사용하지 않았으며 fixture를 유지했다.

## 회귀와 인계

[검증 명령](05-artifacts/checks.json): scheduler 테스트 3개, `cargo check --workspace`, 테스트 기능의 desktop cargo check, UI typecheck/build 및 기존 parity/event 검증이 모두 exit 0. 기존 Vite HMR EPERM 및 Cargo 캐시 정리 권한/STATIC_VCRUNTIME 경고는 별도 환경 경고다.

같은 앱에서 `pnpm --dir crates/desktop/ui test:native --no-build --spec=input.spec.mjs` 7개와 `pnpm --dir crates/desktop/ui test:native --no-build --spec=save.spec.mjs` 6개가 통과했다. [입력](05-artifacts/input-regression/results.jsonl), [저장](05-artifacts/save-regression/results.jsonl). 06번은 이 시간대/timeout/preview 및 초안·저장 계약을 유지하면서 운영 상태를 처리한다.
