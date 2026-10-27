# 명령 편집 안전성

`prerequisite_ready: true` — 02번 구현과 필수 검증을 완료했다.

## 수정 전 재현

- 01번 실제 WebView에서 `run start -- --port 3000`의 3000만 4000으로 변경하자 `-- --port 4000 run start`로 바뀌었다.
- 별도 격리 sentinel 파일을 실행 명령으로 등록했다. 설정 창을 열기만 해도 sentinel 실행 마커가 생겼다. N-R 재현.
- 닫히지 않은 따옴표를 입력한 상태에서 설정 적용이 활성화됐고, `cmd_update_process_config`에 이전 `original` 인수가 전달됐다. 이 호출은 N-M으로 가로채 실제 저장은 하지 않았다.
- 명령: 저장소 루트에서 `pnpm --dir crates/desktop/ui test:native --no-build --spec=command-baseline.spec.mjs`.
- 원본 실행 자료: `/private/var/folders/qm/x2f4lqqj5ms9n3z4f_gtc2140000gn/T/msv-desktop-ux-CdDbcq/output`.

## 변경 범위

인수의 원래 순서와 빈 값을 보존하고, 입력 중인 원문과 유효성을 프로세스·작업·워크플로·exec 상태 확인의 저장/실행 소비자까지 전달한다. 실행 파일·도움말 조회는 명시적인 실행 버튼으로 제한한다. 공개 DTO·invoke 명령·낙관적 동시성 키는 바꾸지 않는다.

## 구현과 결과

- `rowsToArguments`의 scope 정렬과 암묵적인 토큰 정규화를 제거했다. `--`, 빈 인수, 빈 옵션 값, 붙은 옵션 값 및 순서가 유지된다.
- 실행 파일과 argv 입력을 분리하고 첫 인수가 실행 파일 이름과 같아도 제거하지 않는다. 원문을 입력할 때마다 구문을 확인하며 유효하지 않은 값은 상위 저장/실행을 막는다.
- 프로세스·작업·워크플로 명령·exec 상태 검사에 초안과 유효성을 전달했다. 탭이나 노드를 바꿔도 잘못된 원문과 저장 차단이 유지된다.
- mount·입력·실행 파일 변경에서 실행자/도움말 조회를 제거했다. 실행자 검색의 버전 조회와 도움말은 각각 명시적 버튼으로 실행한다.
- 도움말 결과가 이전 초안에 해당하면 반영하지 않는다. 결과 받기 취소는 결과를 폐기하고 백엔드의 기존 제한 시간까지 정리를 기다리는 동작이다. 즉시 OS 프로세스 종료를 약속하지 않으며 UI에 최대 12초 제한을 표시한다. 실제 느린 도움말은 기존 4초 제한 후 종료됨을 확인했다.

| 시나리오 | 모드 | 확인 |
| --- | --- | --- |
| CMD-01 | N-R | 3000→4000만 수정, 저장 재조회와 실제 자식 argv·환경변수 일치 |
| CMD-02 | N-R | 빈 문자열·쉼표·유니코드·따옴표·반복 옵션·실행자 이름 같은 첫 인수의 실제 전달 |
| CMD-03 | N-M + N-R | 잘못된 원문으로 저장 요청 0회, 탭 왕복 후 보존, 복원 뒤 실제 수정 저장 |
| CMD-04 | N-R | 편집만으로 sentinel 실행 0회, 명시적 도움말 실행 및 키보드 후보 선택 |
| CMD-05 | N-R | 느린 도움말 결과 폐기, 새 초안 유지, 제한 시간 후 프로세스 정리 |
| CMD-06 | N-R | 작업·워크플로·exec 상태 검사에서 잘못된 입력 차단과 정상 값 저장 재조회 |

실제 실행 결과는 [결과 원장](02-artifacts/native/results.jsonl), [자식 프로세스가 받은 값](02-artifacts/native/command-final-consumer.json), [도움말 정리](02-artifacts/native/help-cleanup.json), [앱 식별](02-artifacts/native/identity.json), [명령 명세](02-artifacts/native/manifest.json), [정리 결과](02-artifacts/native/cleanup.json)를 따른다. 결과 원장의 짧은 공통 명령 표기보다 명령 명세의 `--spec` 포함 원문을 사용한다.

## 실행한 검증

작업 디렉터리는 저장소 루트이며 마지막 두 명령은 `crates/desktop/ui`에서 실행했다.

```sh
pnpm --dir crates/desktop/ui typecheck
pnpm --dir crates/desktop/ui build
pnpm --dir crates/desktop/ui test:native --spec=command.spec.mjs
pnpm --dir crates/desktop/ui test:native --no-build --spec=command.spec.mjs
node tests/command-model.test.mjs
node tests/operations-parity.test.mjs
pnpm test:follow-events
```

모두 종료 코드 0. 순수 모델 검증은 28개 왕복 사례와 순서/구문 검사를 포함한다. 기존 parity/event 테스트의 Vite HMR `0.0.0.0:24678` 바인딩 권한 경고는 R 검증의 한계로 유지하며 네이티브 결과와 구분한다. Rust 코드는 02에서 변경하지 않았다.

## 다음 단계와 한계

03번은 이 argv/초안 계약을 유지하며 반복 항목·숫자·정책·환경변수 입력을 수정한다. 실제 OS IME 관찰은 아직 수행하지 않았으며 이 문서의 N-R은 OS IME 증거가 아니다. 06번에서는 기존 invoke 오류 정규화가 평탄한 `{code,message}`를 일반 오류 문구로 감추는 현상을 확인해야 한다. 진단용 원본은 `details`에 남아 있어 테스트 데이터의 5년 예약 한도 초과를 구분할 수 있었다.

## 10단계에서 발견한 포커스 경합

이전 입력란의 100/150ms 지연 blur가 다시 집중한 입력란 또는 다른 입력란의 후보를 닫는 경합을 수정했다. 지연 처리에서 현재 대상 ID와 실제 활성 요소를 확인한다. 03의 INPUT-02는 180ms 뒤에도 후보를 선택할 수 있는지 재현하며, 수정 전 실패 기록은 10-artifacts에 보존한다. 이 보완의 native 재검증이 완료되면 준비 상태를 복구한다.

## 격리된 최종 자동 회귀

사용자 입력과 분리한 최신 호스트에서 해당 자동 사례가 모두 통과했다. 실제 OS 관찰과 합성 이벤트 재생은 [10단계](10-native-gates.md)에 구분한다. 포커스 경합 보완의 재검증은 완료했으며, 전체 브리프셋의 새 OS 관찰 게이트는 별도로 남아 있다.
