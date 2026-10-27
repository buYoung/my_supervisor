# 반복 입력과 편집 초안

`implementation_ready: true` · `automatic_ready: true` · `prerequisite_ready: true` — 구현과 필수 검증을 완료했다. 변경된 입력의 자동 8개를 재검증했고 실제 IME·포커스는 격리 VM에서 확인했다. 영향 없는 04–09의 검증과 기존 OS 관찰은 재사용한다. 최신 빌드와 판정은 [10단계](10-native-gates.md)를 따른다.

## 구현과 보존한 계약

- `RepeatedField`로 작업 의존성과 감시 경로를 행 단위로 편집한다. 명확한 등록 이름만 쉼표로 확정하며, 쉼표가 포함된 이름은 전체 후보 선택으로 보존한다. 경로는 쉼표를 분할하지 않는다. 여러 항목 붙여넣기는 확인 목록을 거치고 삭제는 편집 세션 안에서 되돌린다.
- `FormDraftsProvider`와 `NumberField`가 숫자의 빈 문자열, 잘못된 환경변수 이름, 비활성 정책별 입력을 메모리에 보관한다. 잘못된 활성 입력은 저장을 차단한다. 트리거·HTTP/TCP·감시·메모리·상태 검사 전환은 기존 초안을 복원하고, 꺼진 정책은 DTO에 포함하지 않는다.
- 환경변수 추가 행은 검색 중에도 보이며 이름에 초점을 준다. 값은 여러 줄을 편집할 수 있고 가져오기는 추가·변경·삭제·충돌을 표시한다. 충돌 선택은 초안에만 적용하며, 저장 기준은 프로세스·작업·워크플로 단계의 원본이다.
- 지원하지 않는 심볼릭 링크 순회를 제출할 수 없고, 감시 절대 경로·양수 지연·상태 검사 시간 관계·순차 재시작 준비 상태와 용량을 저장 전에 검증한다. 경로 제외는 절대 경로 접두사 규칙임을 표시한다.
- 기존 `OperationsClient`, DTO, invoke/HTTP 키, 식별자와 expected-config/revision 계약은 변경하지 않았다. 워크플로 단계의 임의 식별자를 초안 키로 사용할 때 구분자 충돌을 막도록 인코딩한다. 비밀 값의 영구 저장이나 새 런타임 의존성은 추가하지 않았다.

## 기준과 네이티브 결과

기준 실행은 `pnpm --dir crates/desktop/ui test:native --no-build --spec=input-baseline.spec.mjs`이며 [원본 관찰](03-artifacts/baseline/)에 저장했다. 숫자를 비우면 0, 상태 검사 URL 및 120초 간격은 종류 전환 후 기본값, 검색 중 추가한 환경변수는 숨겨지고 잘못된 이름은 이전 이름으로 복원됐다.

03단계 당시 자동 실행 명령은 저장소 루트(`/Users/buyonglee/Documents/work/private/my_supervisor`)의 `pnpm --dir crates/desktop/ui test:native --spec=input.spec.mjs`다. 실제 macOS 15.7.4, debug/embedded Tauri 앱과 unmocked invoke를 사용했다. WebKit 605.1.15, `tauri://localhost`, revision `748537710413f224be9f25a65d114426b33c0770`, 빌드 소스 SHA-256 `3f3e5ad9ffeeb3f6dd416a2ad398d0323a7f8086f31635ebe2ddd5049da4b7f4`다. 실행 스크립트 내부 cwd는 `crates/desktop/ui`이며 정확한 명령과 격리 경로는 [manifest](03-artifacts/native/manifest.json), 실제 앱 정보는 [identity](03-artifacts/native/identity.json)에 기록했다.

| 사례 | 결과 | 최종 소비자·경계 |
|---|---|---|
| INPUT-01 | N-R 통과 | 쉼표와 Enter 뒤 초점 이동, 저장된 의존성 이름 일치 |
| INPUT-02 | N-R 통과 | 빈 구분자·중복·자기 참조·미등록 값 차단, 중간 행 편집, Unicode/쉼표 이름, 붙여넣기 확인 후 실제 저장 |
| INPUT-03 | N-R 통과 | 쉼표가 있는 절대 경로를 한 항목으로 보존, 다음 행 추가, 상대 경로 거부 |
| INPUT-04 | N-R 통과 | 숫자 빈 초안 유지, 5000ms/3000ms 실제 저장, 로컬 HTTP 검사 소비자가 각각 4999.56ms/3002.51ms로 관찰 |
| INPUT-05 | N-R 통과 | HTTP/TCP/비활성 전환과 120초 트리거 복원, 비활성 정책은 저장에서 제외 |
| INPUT-06 | N-R 통과 | 검색 중 추가·잘못된 이름·중복·삭제 되돌리기·파일 충돌 및 여러 줄 값 보존 후 저장 |
| INPUT-07 | 현재 N-R + O 통과 | VM 실제 한글 입력에서 첫 Enter는 1행 유지, 두 번째는 2행 이동. 저장된 트리거는 one_shot 유지. [최종 실제 OS 결과](10-artifacts/os-vm/os-ime-clean/results.jsonl) |
| INPUT-08 | N-R 통과 | 심볼릭 링크 제한과 순차 재시작 선행 조건 안내·최종 저장 |

[사례별 결과와 스크린샷](03-artifacts/native/results.jsonl), [검사 시간](03-artifacts/native/health-timings.json), [정리 결과](03-artifacts/native/cleanup.json)를 함께 확인한다. HTTP 시간은 저장된 정확한 값을 소비하는지 확인하기 위한 허용 오차를 사용하며 일반적인 성능 목표가 아니다.

## IME 관찰

명령 `pnpm --dir crates/desktop/ui test:native --no-build --spec=ime-observe.spec.mjs --observe`로 격리된 작업을 열었다. CUA `pressKey` 입력은 한글이 표시돼도 조합 이벤트가 없거나 자모가 분리되는 결과를 냈다. [도구 입력 기록](03-artifacts/ime-tool-attempt/native-ime.json)은 O 통과 증거로 인정하지 않는다. 이후 사용자가 실제 키보드로 첫 Enter는 조합 확정, 두 번째는 다음 행이라고 확인했다. [원본 이벤트와 관찰](03-artifacts/native-ime/native-ime.json)에 keyCode 229와 insertReplacementText 및 최종 한글/2행/둘째 행 초점이 기록됐다. 해당 WebView에서는 compositionstart/end가 발생하지 않아 원래 자동 판정은 실패했다. 이를 숨기지 않고 [원래 결과](03-artifacts/native-ime/results.jsonl)를 보존했다. 실제 macOS 신호를 지원하도록 판정을 보완한 뒤 [원본 증거 재검증](03-artifacts/native-ime-validation.json)이 exit 0으로 통과했다. 브라우저 합성 이벤트로 관찰을 대신하지 않았다.

## 검증·정리·후속 인계

[검증 명령 기록](03-artifacts/checks.json): UI typecheck와 일반 build가 exit 0. `node tests/operations-parity.test.mjs`와 `pnpm test:follow-events`도 각각 exit 0이며 R 증거다. 두 기존 검증은 Vite HMR `listen EPERM 0.0.0.0:24678` 경고를 출력했다. 이를 네트워크 통합 검증 성공으로 확대하지 않는다. 네이티브 빌드에는 기존 Cargo 캐시 정리 권한 및 `STATIC_VCRUNTIME` 폐기 예정 경고가 있었고 빌드는 성공했다.

모든 완료된 실행의 정리 결과에서 전용 포트 종료, 테스트 앱 종료, 남은 소유 프로세스 없음이 확인됐다. fixture root는 task-owned 임시 디렉터리이며 사용자 설정·작업·서비스는 사용하지 않았다. 이 단계에 Rust 변경은 없다.

04번은 이 초안·유효성 계약을 유지하며 저장/닫기/탐색과 버전 충돌을 처리한다. 사용자 관찰 후 테스트 앱과 전용 포트가 종료됐고 남은 소유 프로세스는 없다. 서명·설치 릴리스 및 OS 재시작 복구는 확인 범위가 아니다.

## 10단계에서 발견한 포커스 경합

이전 입력란의 100/150ms 지연 blur가 다시 집중한 입력란 또는 다른 입력란의 후보를 닫는 경합을 수정했다. 지연 처리에서 현재 대상 ID와 실제 활성 요소를 확인한다. 03의 INPUT-02는 180ms 뒤에도 후보를 선택할 수 있는지 재현하며, 수정 전 실패 기록은 10-artifacts에 보존한다. 해당 자동 회귀와 [실제 VM 포커스 관찰](10-artifacts/os-vm/native-focus-observations.json)이 모두 완료돼 준비 상태를 복구했다.

## 격리된 최종 자동 회귀

사용자 입력과 분리한 비활성 자동 호스트에서 수정 영향이 있는 입력 7개와 IME 재생 1개가 통과했다. VM에서 composition 이벤트 없이 같은 값의 insertReplacementText와 Enter를 보내는 WebKit 경로를 확인해, RepeatedField가 같은 입력 프레임의 조합 확정 Enter만 막도록 수정했다. 키 해제·blur·다음 프레임에서 보호 상태를 해제해 두 번째 Enter는 그대로 동작한다. 공개 저장 계약과 DTO는 유지했다.

macOS 15.7.7 / VirtualMac2,1의 [최종 실제 IME 관찰](10-artifacts/os-vm/os-ime-clean/native-ime.json)과 [전체 사례 통과](10-artifacts/os-vm/os-ime-clean/results.jsonl)는 첫 Enter 1행, 두 번째 Enter 2행, 저장된 one_shot 유지를 확인했다. RepeatedField와 CommandEditor의 실제 Shift+Tab/Tab 포커스 복귀도 확인했다. 사용자 화면·입력·클립보드를 사용하지 않았고 VM은 정지했다. 합성 재생과 O 관찰, 실패 이력, 정확한 빌드 및 명령은 [10단계](10-native-gates.md)에 구분한다.
