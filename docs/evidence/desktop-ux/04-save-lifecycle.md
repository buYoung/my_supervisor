# 저장과 탐색 수명주기

`prerequisite_ready: true` — SAVE-01~06과 명령·입력 회귀 검증을 완료했다.

## 변경과 계약

공통 편집 세션이 dirty/valid/saving/error를 관리하고 Escape, 닫기, 배경, 취소, 사이드바, 목록 이동, 일반 내부 링크 및 브라우저 뒤로 가기에 동일한 선택을 제공한다. 변경이 없으면 즉시 닫고, 변경이 있으면 계속 편집하거나 명시적으로 버린다. 본문 해시 이동은 이탈이 아니다. 검색과 선택은 메모리에 보관한다.

프로세스·작업 저장은 제출 스냅샷과 중복 제출 방지 참조를 사용한다. 저장 중 입력을 잠그고, 실패하면 초안을 유지한다. 흐름은 늦은 응답으로 새 초안을 교체하지 않으며, 저장 중 이탈 안내도 완료 상태에 맞게 갱신된다. 환경변수 가져오기는 초안 반영으로 표시하고 실행 중 프로세스 적용의 재시작 효과를 설명한다.

작업 설정 저장 뒤 최신 흐름 버전을 조회해 편집 중인 흐름과 병합한다. 동기화를 기다리는 동안 생긴 입력도 최신 초안을 기준으로 처리한다. 겹친 변경은 필드별로 내 변경/최신 저장본 중 하나를 선택해야 한다. 프로세스의 기존 `expected`와 흐름의 `expected_revision`을 유지하며 최신 번호만 바꿔 재전송하지 않는다. 식별자와 배열 순서가 충돌하면 명시적으로 해결한다. 비활성 초안은 명시적으로 최신 저장본을 선택해 버리지 않는 한 유지한다.

## 기준과 최종 결과

기준 명령은 저장소 루트(`/Users/buyonglee/Documents/work/private/my_supervisor`)의 `pnpm --dir crates/desktop/ui test:native --no-build --spec=save-baseline.spec.mjs`다. [관찰](04-artifacts/baseline/save-baseline.json)에서 dirty Escape 닫힘, 설정 저장 후 이전 버전 표시, 후속 편집을 덮는 지연 응답을 재현했다.

최종 앱은 macOS 15.7.4, debug/embedded Tauri, WebKit 605.1.15다. revision `748537710413f224be9f25a65d114426b33c0770`, 빌드 소스 SHA-256 `0cb2bb33b1bcfbc149cd115c9a28d579d618ef2534809e6eaf721c0b6d34777a`. [앱 정보](04-artifacts/native/identity.json)와 [실행 명령·격리 경로](04-artifacts/native/manifest.json)를 보존했다.

| 사례 | 실제 확인 |
|---|---|
| SAVE-01 | N-R: 닫기 4경로, dirty 유지/명시적 버림, clean 닫기, 사이드바·목록·실행 로그 링크·브라우저 Back, 해시 이동, 검색·선택 복원 |
| SAVE-02 | N-R: 잘못된 명령·빈 숫자·미등록 의존성에서 저장 차단, 기존 저장값 보존 |
| SAVE-03 | N-R: 작업 설정 v1→v2와 로컬 흐름 병합, 외부 v4 충돌 후 명시적 선택으로 v5 저장; 프로세스 expected 충돌 해결 및 중지 상태 유지 |
| SAVE-04 | N-R: 실제 저장 응답 지연 동안 입력 잠금과 이탈 안내, 실제 오래된 버전 요청의 거절 후 초안 유지·재시도 |
| SAVE-05 | N-R: 이탈 취소와 두 번 누르기에도 실제 적용 호출 1회, 프로세스 시작 기록 2개(최초 시작+재시작 1회) |
| SAVE-06 | N-R: 가져오기 후 저장 전 실제 환경변수 미변경, 초안·재시작 효과 표시 |

전체 실행 명령은 저장소 루트의 `pnpm --dir crates/desktop/ui test:native --spec=save.spec.mjs`이며 [사례 결과](04-artifacts/native/results.jsonl), [버전·재시작 기록](04-artifacts/native/save-results.json), [정리](04-artifacts/native/cleanup.json)를 남겼다.

SAVE-04의 최종 실패 증거는 같은 루트에서 `pnpm --dir crates/desktop/ui test:native --no-build --spec=save.spec.mjs --case=SAVE-04`로 별도 실행했다. IPC는 모의 처리하지 않았으며, 요청 버전을 한 단계 이전 값으로 지정해 실제 백엔드 거절을 받은 뒤 응답 전달만 지연했다. [실제 오류](04-artifacts/real-failure/save-failure.json), [결과](04-artifacts/real-failure/results.jsonl), [정리](04-artifacts/real-failure/cleanup.json). 초기 전체 실행의 직접 예외 발생 분기는 N-M 보조 관찰이며 최종 실패 통과 근거로 사용하지 않는다.

## 회귀와 제한

- 명령 회귀: `pnpm --dir crates/desktop/ui test:native --no-build --spec=command.spec.mjs`, 6개 통과. CMD-03만 명시적 N-M이며 나머지는 N-R. [결과](04-artifacts/command-regression/results.jsonl)
- 입력 회귀: `pnpm --dir crates/desktop/ui test:native --no-build --spec=input.spec.mjs`, 자동 7개 N-R 통과. [결과](04-artifacts/input-regression/results.jsonl). INPUT-07의 실제 키보드 관찰은 03번에 별도로 보존했다.
- `pnpm --dir crates/desktop/ui typecheck`, `pnpm --dir crates/desktop/ui build`, `node crates/desktop/ui/tests/reconcile-draft.test.mjs` 모두 exit 0. `crates/desktop/ui`에서 기존 parity/event 검증도 exit 0. [명령 기록](04-artifacts/checks.json)
- Vite HMR `listen EPERM 0.0.0.0:24678`은 기존 R 검증의 환경 경고다. Cargo 캐시 정리 권한과 STATIC_VCRUNTIME 경고는 네이티브 빌드가 성공해도 남아 있다. R 결과를 네트워크 통합 검증으로 확대하지 않는다.
- [중간 시도](04-artifacts/attempts/)에는 선택자 오류, 드라이버의 fieldset 활성 판정 차이와 수정 전 해시 이동 검증 실패를 남겼다. 최종 사례는 실제 WebView의 `:disabled`와 저장값을 확인한다.
- 모든 완료 실행에서 전용 앱·포트·소유 프로세스 정리를 확인했다. 사용자 작업/서비스를 사용하지 않았다. 외부 앱 재시작 뒤 초안 복원은 제공하지 않으며, 네이티브 닫기/트레이 OS 관찰은 10번에서 별도 확인한다. Rust·공개 API 변경은 없다.

05번은 이 저장/초안 계약을 유지하면서 시간대·제한 시간과 미리보기 의미를 맞춘다. [실행 소스 해시](04-artifacts/source-hashes.json)를 함께 보존한다.
