# 로그 탐색과 원본

`prerequisite_ready: true` — LOG-01~07과 관련 검사가 통과했다.

## 기준과 변경

[기준 실행](08-artifacts/baseline/log-baseline.json)에서 실제 INFO 원본이 있는데 ‘전체 로그’ 링크가 attention으로 조회했다. 각 진입 링크가 source/run/step와 전체 수준·전체 기간을 명시하도록 했다. 조회 조건, 정렬, 그룹, 페이지, 스냅샷과 읽던 위치는 세션 및 안전한 URL 매개변수로 유지하며 원본 내용이나 환경변수는 URL에 넣지 않는다.

각 행은 표시 시간대의 날짜와 시간을 보여 준다. 자동 갱신은 5초마다 새 기록을 확인해 알리고, 기존 결과를 자동으로 바꾸지 않는다. 원본/문맥 보기, 펼친 그룹, 이전 페이지에서는 중지 이유를 표시한다. 그룹 원본은 50건 단위로 앞뒤 이동한다. 기본 목록도 한 번에 50행을 표시하고 소스 목록은 기존 페이지 계약을 쓴다.

프로세스 미리보기와 탐색기가 같은 원본 창을 사용한다. 원문과 정리한 JSON을 구분하고, 복사 결과는 창 안에 표시한다. 내보내기는 고정한 until/snapshot과 5000건 단위 조회를 사용한다. 작업 버퍼는 페이지별 Blob으로 나누며, 반복 페이지·기록 수 불일치·일부 소스 누락은 완료로 처리하지 않는다.

## 실제 검증

저장소 루트 `/Users/buyonglee/Documents/work/private/my_supervisor`에서 `pnpm --dir crates/desktop/ui test:native --spec=log.spec.mjs`로 빌드 후 `pnpm --dir crates/desktop/ui test:native --no-build --spec=log.spec.mjs`로 7개를 통과했다. revision `748537710413f224be9f25a65d114426b33c0770`, 앱 소스 SHA-256 `bb84f2119c969a3c9fdc02e9105c767c25536474a8f7ac7021336178a621a627`, macOS 15.7.4 / WebKit 605.1.15 debug embedded. 마지막 재실행은 테스트에 대상 전환 검증만 보강했으므로 테스트 소스 해시와 이미 빌드한 앱의 해시를 구분한다.

[식별](08-artifacts/native/identity.json), [정확한 cwd·명령·fixture](08-artifacts/native/manifest.json), [개별 결과](08-artifacts/native/results.jsonl), [값 기록](08-artifacts/native/log-results.json), [정리](08-artifacts/native/cleanup.json).

| 사례 | 실제 확인 |
|---|---|
| LOG-01 | N-R: 프로세스 10,000건 INFO, 작업 및 선택 단계의 INFO를 올바른 source/run/step와 전체 수준으로 조회 |
| LOG-02 | N-R: 실제 여러 날짜의 기록과 표시 행의 YYYY-MM-DD 확인 |
| LOG-03 | N-R: 새 stdout과 종료 상태 기록이 들어와도 펼친 그룹·이전 페이지 불변. 자동 갱신 상태와 새 기록 반영 동작 확인 |
| LOG-04 | N-R: 원본 → 문맥 → 브라우저 Back 후 두 번째 페이지와 스크롤 복원. 작업 A에서 B로 변경하면 A 실행 목록과 행 제거 |
| LOG-05 | N-R: 실제 날짜 범위 원본을 여러 페이지로 내보내고 최소·최대 시각 검증. 읽을 수 없는 테스트 소스 포함 시 다운로드를 완료하지 않고 오류 표시 |
| LOG-06 | N-R: 미리보기/탐색기 원문 일치, 실제 clipboard.writeText의 성공과 전달 원문 일치, 10,000건에서 50행 표시 |
| LOG-07 | N-R: 선택한 프로세스를 실제 삭제한 뒤 조회 오류를 표시. 잘못된 빈 성공 화면 대신 source·재시도 유지 |

내보내기 테스트는 실제 조회로 만들어진 Blob을 다운로드 경계에서 읽고 OS 다운로드는 실행하지 않았다. 전송·백엔드는 대체하지 않았다. 복사는 원래 clipboard.writeText를 호출하는 관찰 래퍼로 전달값을 확인했다. OS 클립보드를 별도로 다시 읽는 시도는 WebKit 응답이 끝나지 않아 제외했으며, 이를 읽기 성공으로 보고하지 않는다. 최종 OS 관찰은 10에서 별도 기록한다.

## 검사와 계약

[검사 결과](08-artifacts/checks.json): navigation 단위 검사, UI typecheck/build와 기존 parity/event 검사가 exit 0. Vite HMR EPERM은 기존 환경 경고다. Rust/백엔드 스냅샷 계약은 변경하지 않았다. custom until을 snapshot_at으로 사용하는 기존 구현을 그대로 사용하며, 조사되지 않은 범위 확장 결함을 만들거나 수정했다고 주장하지 않는다.

[첫 시도](08-artifacts/first-native-attempt/)의 새 로그 수 고정 가정과 Blob fetch 보안 오류는 테스트에서 수정했다. 실제 실행에는 stdout 외에 종료 상태 로그도 생긴다. Blob 자체를 관찰하도록 바꾸었고 프로덕션 CSP는 그대로 두었다. 실패·정상 실행 모두 task-owned root만 사용했다. 09는 이 조회/원문/스냅샷 계약을 유지하고 배치와 대비를 정리한다.
