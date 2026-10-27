# 데스크톱 UX 검증 행렬

`automatic_ready: true` · `prerequisite_ready: true` — 필수 범위를 완료했다. 기존 자동 58개 중 수정 영향이 있는 입력 8개만 다시 통과했고, 나머지 50개와 정리 14회는 재사용했다. 실제 실행 파일 선택·IME·포커스는 격리 VM에서 확인했다.

전체 자동 회귀 당시 바이너리 SHA-256: `de780116a55ec249f8acab5553a4af40ab974ab3e5f2d0e6b5b8d052f1ebc06d`. macOS 15.7.4, WebKit 605.1.15, debug/embedded. 모든 자동 시나리오에서 앱 비활성·창 포커스 없음·실제 키 입력 유입 0을 확인했다. 정확한 명령/cwd/fixture/빌드 식별은 각 results/manifest에 있다. 현재 입력 자동 바이너리는 `97ff56746deb52e8162df06d9e4389c204cbbaf72ebc37814557db6cbf0da3db`, 현재 OS 바이너리는 `9cb6c95571c16eab385948f23abf487cd9fbec09e6d1b92bdc7f2bf3b30d7cf3`다. [제품 소스 일치](10-artifacts/os-vm/final-source-comparison.json)를 확인했다.

R: renderer/정적 검사 · N-M: 명시적 모킹 포함 · N-R: 네이티브 WebView + 실제 invoke/backend · O: 실제 OS 관찰 · O-history: 이전 빌드에서 실제 OS 관찰 · O 재사용: 변경 영향 분석을 근거로 유지하는 기존 관찰이며 현재 빌드 재실행이 아님. 합성 키·클립보드 메모리 fixture를 새 OS 관찰로 표기하지 않는다.

| 시나리오 | 결과 | 현재 증거 종류 | 근거 |
|---|---|---|---|
| INPUT-01 | 통과 | N-R | [기록](10-artifacts/os-vm/ime-frame-checks/input/results.jsonl) |
| INPUT-02 | 통과 | N-R | [기록](10-artifacts/os-vm/ime-frame-checks/input/results.jsonl) |
| INPUT-03 | 통과 | N-R | [기록](10-artifacts/os-vm/ime-frame-checks/input/results.jsonl) |
| INPUT-04 | 통과 | N-R | [기록](10-artifacts/os-vm/ime-frame-checks/input/results.jsonl) |
| INPUT-05 | 통과 | N-R | [기록](10-artifacts/os-vm/ime-frame-checks/input/results.jsonl) |
| INPUT-06 | 통과 | N-R | [기록](10-artifacts/os-vm/ime-frame-checks/input/results.jsonl) |
| INPUT-07 | 통과 | N-R + O | [자동 재생](10-artifacts/os-vm/ime-frame-checks/ime-replay-final/results.jsonl), [실제 IME](10-artifacts/os-vm/os-ime-clean/results.jsonl) |
| INPUT-08 | 통과 | N-R | [기록](10-artifacts/os-vm/ime-frame-checks/input/results.jsonl) |
| FLOW-01 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/flow/results.jsonl) |
| FLOW-02 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/flow/results.jsonl) |
| FLOW-03 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/flow/results.jsonl) |
| FLOW-04 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/flow/results.jsonl) |
| FLOW-05 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/flow/results.jsonl) |
| FLOW-06 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/flow/results.jsonl) |
| FLOW-07 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/flow/results.jsonl) |
| LOG-01 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/log/results.jsonl) |
| LOG-02 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/log/results.jsonl) |
| LOG-03 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/log/results.jsonl) |
| LOG-04 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/log/results.jsonl) |
| LOG-05 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/log/results.jsonl) |
| LOG-06 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/log/results.jsonl) |
| LOG-07 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/log/results.jsonl) |
| CMD-01 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/command/results.jsonl) |
| CMD-02 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/command/results.jsonl) |
| CMD-03 | 통과 | N-M | [기록](10-artifacts/os-vm/automatic/command/results.jsonl) |
| CMD-04 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/command/results.jsonl) |
| CMD-05 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/command/results.jsonl) |
| CMD-06 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/command/results.jsonl) |
| SAVE-01 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/save/results.jsonl) |
| SAVE-02 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/save/results.jsonl) |
| SAVE-03 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/save/results.jsonl) |
| SAVE-04 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/save/results.jsonl) |
| SAVE-05 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/save/results.jsonl) |
| SAVE-06 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/save/results.jsonl) |
| SCHEDULE-01 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/schedule/results.jsonl) |
| SCHEDULE-02 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/schedule/results.jsonl) |
| SCHEDULE-03 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/schedule/results.jsonl) |
| SCHEDULE-04 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/schedule/results.jsonl) |
| SCHEDULE-05 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/schedule/results.jsonl) |
| SCHEDULE-06 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/schedule/results.jsonl) |
| STATE-01 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/state/results.jsonl) |
| STATE-02 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/state/results.jsonl) |
| STATE-03 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/state/results.jsonl) |
| STATE-04 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/state/results.jsonl) |
| STATE-05 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/state/results.jsonl) |
| STATE-06 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/state/results.jsonl) |
| STATE-07 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/state/results.jsonl) |
| STATE-08 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/state/results.jsonl) |
| VISUAL-01 | 통과 | N-M | [기록](10-artifacts/os-vm/automatic/visual/results.jsonl) |
| VISUAL-02 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/visual/results.jsonl) |
| VISUAL-03 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/visual/results.jsonl) |
| VISUAL-04 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/visual/results.jsonl) |
| VISUAL-05 | 통과 | N-M | [기록](10-artifacts/os-vm/automatic/visual/results.jsonl) |
| VISUAL-06 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/visual/results.jsonl) |
| HARNESS-01 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/harness/results.jsonl) |
| HARNESS-02 | 통과 | N-M | [기록](10-artifacts/os-vm/automatic/harness/results.jsonl) |
| HARNESS-03 | 통과 | N-R | [기록](10-artifacts/os-vm/cleanup/runs.json) |
| HARNESS-04 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/harness/results.jsonl) |
| HARNESS-05 | 통과 | R | [기록](10-artifacts/checks/normal-build-audit.json) |
| GATE-01 | 통과 | N-R + O + O 재사용 | [최종 판정](10-native-gates.md), [원장](scenario-ledger.json) |
| GATE-02 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/harness/results.jsonl) |
| GATE-03 | 통과 | O + O 재사용 | [파일 선택](10-artifacts/os-vm/os-file-pass/os-observations.json), [IME](10-artifacts/os-vm/os-ime-clean/native-ime.json), [포커스](10-artifacts/os-vm/native-focus-observations.json), [재사용](10-artifacts/os-vm/os-reuse-analysis.json) |
| GATE-04 | 통과 | R + N-R | [기록](10-artifacts/checks/results.json) |
| GATE-05 | 통과 | R + N-R | [기록](10-artifacts/os-vm/cleanup-summary.json) |
| GATE-06 | 통과 | R | [기록](10-artifacts/os-vm/automatic/isolation/manifest.json) |
| ISOLATION-01 | 통과 | N-R | [기록](10-artifacts/os-vm/automatic/isolation/isolation-proof.json) |

GATE-01/03의 남은 범위를 격리 VM에서 관찰했다. 실행 파일 선택·취소와 IME는 전체 사례 통과를, 두 입력란 포커스는 신뢰된 Tab 이벤트와 350ms 뒤 화면을 근거로 한다. 기존 폴더·클립보드·트레이 관찰은 [빌드 정보와 영향 분석](10-artifacts/os-vm/os-reuse-analysis.json)으로 재사용한다. 실패한 중간 실행은 그대로 보존하며 최종 IME는 os-ime-clean 결과만 채택한다. 사용자 화면·입력·클립보드는 사용하지 않았고 [VM 종료](10-artifacts/os-vm/vm-stop.json)를 확인했다.

과거 실패와 입력 간섭 실행은 현재 통과 근거에서 제외한다. 별도 이름 변경의 기존 외래 키 문제는 [범위 밖 관찰](10-artifacts/incidental-rename.md)에 남긴다.
