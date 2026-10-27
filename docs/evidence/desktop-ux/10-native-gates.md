# 데스크톱 UX 구현과 최종 검증

`implementation_ready: true` · `automatic_ready: true` · `prerequisite_ready: true`

브리프셋 01–10의 필수 범위를 완료했다. 기존 자동 58개 중 IME 수정의 영향을 받는 입력 8개만 다시 확인했고 모두 통과했다. 나머지 50개와 정리 검사 14회는 재사용했다. 실행 파일 선택·한글 IME·입력란 포커스는 격리 macOS VM에서 관찰했다. 기존 폴더·클립보드·트레이 관찰은 빌드 정보와 변경 영향 분석을 근거로 유지한다.

## 이번에 수정한 동작

VM의 WebKit은 한글 확정 시 composition 이벤트 없이 같은 값의 `insertReplacementText` 다음에 Enter를 전달했다. 이 경로에서 첫 Enter가 행을 추가하는 문제를 확인해 `RepeatedField`를 수정했다. 같은 입력 프레임의 조합 확정 Enter만 막고, 키 해제·blur·다음 프레임에서 보호 상태를 해제한다. 두 번째 Enter의 행 추가는 유지한다. 작업 의존성과 감시 경로가 같은 컴포넌트를 사용하며 공개 DTO, 저장 계약, 런타임 의존성은 바꾸지 않았다.

기존 IME 재생 검사는 별도로 기록된 두 Enter 경계가 있는 현재 VM 이벤트를 사용한다. 한 키 동작의 replacement와 keydown은 함께 재생하고 keyup 뒤 렌더링을 기다린다. 첫 Enter 1행·두 번째 Enter 2행, 포커스, 저장된 `one_shot` 유지, 실제 호스트 키 입력 0회 검사를 모두 유지한다. 합성 재생은 새로운 OS 관찰로 계산하지 않는다.

## 자동 검증

| 범위 | 결과와 근거 |
|---|---|
| 기존 자동 58개 | [기존 실행 기록](10-artifacts/os-vm/automatic/runs.json). 영향 없는 50개는 반복하지 않았다. |
| 변경 영향이 있는 입력 8개 | `input.spec.mjs` 7개와 `ime-replay.spec.mjs` 1개 통과. [입력 결과](10-artifacts/os-vm/ime-frame-checks/input/results.jsonl), [IME 재생 결과](10-artifacts/os-vm/ime-frame-checks/ime-replay-final/results.jsonl), [명령·cwd·실패 및 최종 결과](10-artifacts/os-vm/ime-frame-checks/native-runs.json). |
| UI typecheck·일반 build | 수정 후 모두 exit 0. [검사 기록](10-artifacts/os-vm/ime-frame-checks/checks.json). |
| 기존 parity/event·Rust 검사 | 이후 관련 변경이 없어 [기존 결과](10-artifacts/os-vm/checks/results.json)를 유지한다. 제품 단위 검사와 scheduler preview도 [기존 기록](10-artifacts/checks/results.json), [preview 기록](10-artifacts/checks/scheduler-preview.json)을 유지한다. |
| 정리·입력 격리 | [기존 정리 14회](10-artifacts/os-vm/cleanup-summary.json)를 반복하지 않았다. 추가 실행은 각 실행의 종료 결과만 확인했다. [입력 실행 정리](10-artifacts/os-vm/ime-frame-checks/input/cleanup.json), [재생 실행 정리](10-artifacts/os-vm/ime-frame-checks/ime-replay-final/cleanup.json). 자동 실행은 비활성·투명·마우스 입력 무시 상태이며 실제 키 입력은 0회다. |

자동 명령의 작업 디렉터리는 `/Users/buyonglee/Documents/work/private/my_supervisor`다.

```text
pnpm --dir crates/desktop/ui test:native --spec=input.spec.mjs
pnpm --dir crates/desktop/ui test:native --no-build --spec=ime-replay.spec.mjs
pnpm --dir crates/desktop/ui typecheck
pnpm --dir crates/desktop/ui build
```

## 새로 확인한 실제 OS 동작

macOS 15.7.7 (24G720), arm64, `VirtualMac2,1`, WebKit 605.1.15에서 기존 VM의 가상 키보드·마우스만 사용했다. fixture는 게스트의 작업 전용 임시 폴더에 생성했다. 명령 작업 디렉터리는 `/Users/admin/msv/repo`이며 실행기 내부 cwd는 `/Users/admin/msv/repo/crates/desktop/ui`다.

```text
node crates/desktop/ui/tests/e2e/run-native.mjs --os-observe --no-build --spec=native-gates.spec.mjs --case=GATE-10
node crates/desktop/ui/tests/e2e/run-native.mjs --os-observe --no-build --spec=ime-observe.spec.mjs
```

| 범위 | 실제 결과 |
|---|---|
| 실행 파일 선택·취소 | 취소 후 `/bin/sleep` 유지. 실제 파일 선택기에서 `/usr/bin/printf`를 선택하고 저장한 뒤 `args: ["600"]`, `state: stopped`, `pid: null` 확인. [최종 소비자](10-artifacts/os-vm/os-file-pass/os-observations.json), [GATE-10 통과](10-artifacts/os-vm/os-file-pass/results.jsonl). 이후 RepeatedField 수정은 이 선택·저장 경로에 영향이 없다. |
| 한글 IME | 첫 Enter 뒤 `한글`·1행·`선행 작업 1` 포커스, 두 번째 Enter 뒤 2행·`선행 작업 2` 포커스. 저장된 작업 트리거는 `one_shot` 그대로다. [원본 이벤트](10-artifacts/os-vm/os-ime-clean/native-ime.json), [INPUT-07 전체 사례 통과](10-artifacts/os-vm/os-ime-clean/results.jsonl), [실제 getJobConfig 저장 트리거 단언](../../../crates/desktop/ui/tests/e2e/ime-observe.spec.mjs). |
| RepeatedField 포커스 | 실제 Shift+Tab/Tab으로 돌아온 뒤 350ms 시점에도 두 번째 입력란의 포커스와 후보 목록 유지. [관찰](10-artifacts/os-vm/native-focus-observations.json), [화면](10-artifacts/os-vm/os-ime-frame/repeated-native-focus-return.png). |
| CommandEditor 포커스 | 명시적으로 실행한 `/usr/bin/tar` 도움말 후보를 연 뒤 실제 Shift+Tab/Tab 왕복. 350ms 뒤 옵션 입력란 포커스와 `-c`, `-b`, `-f` 후보 유지. [관찰](10-artifacts/os-vm/native-focus-observations.json), [화면](10-artifacts/os-vm/os-ime-frame/command-native-focus-return.png). 명령 초안은 저장·실행하지 않았다. |

포커스 관찰을 포함한 앞선 IME 세션은 추가 Escape가 미저장 확인창을 열어 전체 사례 판정에 실패했다. [원래 실패](10-artifacts/os-vm/os-ime-frame/results.jsonl)를 보존하며 그 세션 전체를 통과로 계산하지 않는다. 포커스의 두 관찰 지점만 신뢰된 Tab 이벤트와 화면으로 인정한다. 최종 IME는 추가 조작 없이 별도 실행한 `os-ime-clean`의 전체 통과 결과를 사용한다. 비활성 창·기록 전달·권한 제한으로 끝난 시도도 [실패 기록 설명](10-artifacts/os-vm/completion-attempts/README.json)에 구분했다.

## 기존 OS 증거 재사용

재사용은 현재 빌드로 다시 실행했다는 뜻이 아니다. 이전 관찰의 바이너리·명령·플랫폼을 보존하고 이후 수정이 실행 경로에 미치는 영향을 대조했다. [파일 해시와 경로별 영향 분석](10-artifacts/os-vm/os-reuse-analysis.json)에 판단을 기록했다.

| 범위 | 유지하는 증거와 영향 분석 |
|---|---|
| 폴더 선택·취소·cwd 저장 | `PathField → choosePath(directory=true) → ProcessEditor → 저장`은 지연 blur와 RepeatedField 조합 Enter 수정에 영향받지 않는다. [기존 관찰](10-artifacts/os-picker/os-observations.json)은 취소, 쉼표·한글 경로 저장, 호출 버튼 복귀를 포함한다. |
| 원본 로그 클립보드 | `entry.raw.line → navigator.clipboard.writeText` 제품 경로가 유지됐다. [기존 관찰](10-artifacts/os-clipboard/os-observations.json)의 실제 붙여넣기와 원문 일치를 재사용한다. 자동 검사의 메모리 클립보드를 O 근거로 사용하지 않는다. |
| 창 닫기·트레이 복귀 | 일반 빌드의 `CloseRequested → hide`, `build_tray → show/set_focus` 동작은 유지됐다. [기존 관찰](10-artifacts/os-tray/os-observations.json)의 PID·실행 중 작업·상세 화면 유지와 명시적 취소를 재사용한다. |
| 화면 배치·대비·일반 모달 | CSS와 일반 모달 경로가 유지됐다. [기존 네이티브 측정](09-artifacts/final-native/visual-results.json)과 통과한 자동 VISUAL 기록을 유지하고 변경된 입력란 포커스만 위 VM 관찰로 보완했다. |

기존 폴더·클립보드·트레이 바이너리 SHA-256은 `bae500bfaa42ca675cb904f46b3a646a5c75c77baf7bfde4887f27d7dd9d0893`다. [폴더](10-artifacts/os-picker/manifest.json), [클립보드](10-artifacts/os-clipboard/manifest.json), [트레이](10-artifacts/os-tray/manifest.json)의 원래 식별자를 유지한다. 과거 IME 관찰도 이력으로 남기지만 현재 IME 판정은 새 VM 결과가 담당한다.

## 빌드 귀속과 종료

기준 커밋은 `748537710413f224be9f25a65d114426b33c0770`, 브랜치는 `feat/desktop-parity`다. 최종 OS 바이너리 SHA-256은 `9cb6c95571c16eab385948f23abf487cd9fbec09e6d1b92bdc7f2bf3b30d7cf3`, 소스 SHA-256은 `0f6aca86343c52d790c7c7f89ac8a82b49b4c8ab1a82168bd2074085e2c4dc3b`다. [빌드 기록](10-artifacts/os-vm/final-os-build-identity.json)과 [실행 manifest](10-artifacts/os-vm/os-ime-clean/manifest.json)를 보존한다.

전송한 원본·수정 묶음에서 OS 빌드 소스 306개를 복원해 해시를 재계산하고 현재 파일과 대조했다. 차이는 IME 재생 검사 스크립트 하나이며 제품 소스는 일치한다. [비교 결과](10-artifacts/os-vm/final-source-comparison.json). 자동 빌드는 같은 제품 코드와 별도의 비활성 테스트 프로필을 사용한다. 그 [바이너리·빌드 식별](10-artifacts/os-vm/ime-frame-checks/input/manifest.json)과 [재생 실행 식별](10-artifacts/os-vm/ime-frame-checks/ime-replay-final/manifest.json)을 OS 빌드와 혼동하지 않는다.

추가 실행 동안 사용자 화면·키보드·마우스·클립보드를 사용하지 않았다. VM은 호스트 창·오디오·USB 연결·클립보드 공유 없이 실행했고 공유한 두 폴더는 작업 전용 임시 경로다. [VM 설정](10-artifacts/os-vm/vm-setup.json), [앱·포트 정리](10-artifacts/os-vm/os-ime-clean/cleanup.json), [VM 정지 확인](10-artifacts/os-vm/vm-stop.json)을 남겼다. 임시 VM은 정지 상태로 보존했다. 새로운 자동화 프레임워크나 테스트용 제품 기능을 추가하지 않았다.

필수 범위의 미확인 OS 항목은 없다. [검증 행렬](native-matrix.md)과 [원장](scenario-ledger.json)은 과거 결과 재사용·현재 자동·현재 O를 구분한다. 서명된 설치형 릴리스, installed-owner, 로그인·재부팅, Windows/Linux 인증은 범위 밖이다. 기존 Cargo 캐시·`STATIC_VCRUNTIME` 경고와 parity/event의 Vite HMR `EPERM`은 해당 원본 기록에 유지한다. 별도 이름 변경에서 관찰한 기존 외래 키 오류는 [범위 밖 기록](10-artifacts/incidental-rename.md)에 보존하며 해결됐다고 주장하지 않는다. 커밋·push는 하지 않았다.
