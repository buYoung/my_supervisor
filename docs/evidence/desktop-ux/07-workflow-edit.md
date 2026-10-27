# 흐름 편집과 실행 이력

`prerequisite_ready: true` — FLOW-01~07은 kAWQgI에서 통과했다. 이후 변경은 06의 보조 조회/저장 완료 대기 분리이며 저장 회귀 dBQNUN 6개가 통과했다. 흐름 이력·분기·키보드 코드는 전체 통과 이후 바꾸지 않았다.

## 기준과 구현

[기준 실행](07-artifacts/baseline/flow-baseline.json)은 실행 취소 및 명시적인 초안 실행 표기가 없었다. 기존 fitView 효과는 팔레트, 노드 수, 실행과 버전 변경에 묶여 있었다. 초기 프레임 콜백 대기가 시간 초과된 시도는 통과 증거에서 제외했다.

노드/연결 생성·삭제, 설정 및 결과 분기 변경을 최대 100개의 로컬 프레임으로 되돌리고 다시 적용한다. 노드·연결·분기 ID와 필드 원문을 보존하며 같은 필드의 짧은 연속 입력은 묶는다. 삭제와 출력 제거는 연결을 함께 처리한다. 저장 충돌 해결이나 외부 설정 병합은 기존 이력을 초기화하여 이전 revision을 현재 기반으로 오인하지 않는다. 실제 실행을 되돌리는 기능은 아니다.

키보드로 단계와 연결 대상을 고를 수 있다. 분기 위/아래 버튼은 배열 순서만 바꾸고 route ID와 연결 핸들을 유지한다. 순환 연결 및 시작 단계 연결은 기존 제약을 유지한다. 화면 위치는 편집/실행/버전별로 유지하고, 최초 표시나 명시적인 전체 보기만 맞춘다. 팔레트 변경으로 측정값이 바뀌어도 다시 맞추지 않는다.

‘현재 초안 실행’, ‘이 단계만 실행’, ‘저장된 단계 복사’, ‘참조할 저장된 작업’으로 실제 범위를 표시한다. 복사는 독립된 노드와 연결 ID를 만들고, 참조는 원본 작업의 저장본을 실행한다. 재시도·차단과 결과 분기는 설정 요약을 유지하며 접어 볼 수 있다. 실행/버전의 노드·연결 장식은 편집 보기에 남지 않는다.

## 실제 검증

저장소 루트 `/Users/buyonglee/Documents/work/private/my_supervisor`에서 `pnpm --dir crates/desktop/ui test:native --spec=flow.spec.mjs`를 사용했다. macOS 15.7.4, WebKit 605.1.15, debug/embedded Tauri. 실제 revision·앱 SHA-256·fixture·명령은 [identity](07-artifacts/native/identity.json), [manifest](07-artifacts/native/manifest.json)에 기록한다. [개별 결과](07-artifacts/native/results.jsonl), [그래프/실행 스냅샷](07-artifacts/native/flow-results.json), [정리](07-artifacts/native/cleanup.json).

| 사례 | 관찰 |
|---|---|
| FLOW-01 | N-R: 추가·삭제·설정 변경·연결 대상 변경 후 실행 취소/다시 실행. 실제 저장 후 동일 노드 ID와 native-edge-2 복원 |
| FLOW-02 | N-R: 동시에 일치하는 두 분기를 재정렬하고 실제 실행이 route-second와 edge-second 선택 |
| FLOW-03 | N-R: 의도적으로 축소한 viewport가 팔레트/선택 변경에 유지, 전체 보기로만 변경 |
| FLOW-04 | N-R: 복사한 단계는 copy.txt 생성. 원본 저장본을 바꾼 뒤 참조 실행은 reference.txt 생성 |
| FLOW-05 | N-R: 수정한 argv로 선택 단계만 실행. 저장된 문서와 revision 불변, 나머지는 기존 실행기 계약대로 skipped |
| FLOW-06 | N-R: 실행/버전에서 편집으로 돌아오면 원문은 보존하고 과거 상태·차이 CSS는 사라짐 |
| FLOW-07 | O + N-R: 실제 CUA Return으로 분기 이동과 실행 취소, trusted 이벤트 확인. 연결 선택에서 순환 오류 표시 |

초기 검사에서 복합 선택자 오류와 실제 stdout 끝 줄바꿈을 보정했다. 실행 취소의 값 지우기/입력 분리와 viewport 재적용은 제품에서 수정했다. 실패 기록은 [첫 시도](07-artifacts/first-native-attempt/), [두 번째 시도](07-artifacts/second-native-attempt/), [viewport 수정 전](07-artifacts/viewport-attempt/)에 유지한다.

## 공통 계약 회귀

[검사](07-artifacts/checks.json): draft history 단위 검사, typecheck와 일반 build 통과. [명령 6개](07-artifacts/command-regression/results.jsonl), [입력 7개](07-artifacts/input-regression/results.jsonl), [저장 6개](07-artifacts/save-regression/results.jsonl) 통과. 기존 명령 시나리오의 버튼 선택자만 새 실행 범위 표기로 바꿨다.

저장 회귀가 보조 그룹 조회를 저장 완료 조건으로 기다리는 결함을 찾아냈다. 실제 저장 후 그룹 갱신은 비동기로 처리하고, 지연 테스트는 모든 실제 조회를 같은 gate로 해제하도록 보완했다. 현재 저장 회귀에서는 외부 설정 동기화 중 입력, revision 충돌 및 명시적 병합, 저장 중 잠금, 중복 제출을 다시 확인했다.

공개 DTO/ID/선택 실행/취소/revision 계약과 기존 실행기를 변경하지 않았다. 테스트는 임시 root만 사용하고 실제 사용자 데이터를 수정하지 않았다. 08에 이 구현과 검증 결과를 인계한다. 추가 통합 재실행 vbgHyN에서는 Mac 잠금으로 OS 입력을 보낼 수 없었다. 이 추가 실행을 통과로 세지 않는다. 잠금 해제를 요청했으며, 최종 10의 현 앱 OS 재검증은 그 상태에 의존한다. 기존 실제 키보드 관찰은 위 전체 통과 실행과 분리해 유지한다.
