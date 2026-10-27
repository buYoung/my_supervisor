# 화면 배치와 접근성

`prerequisite_ready: true` — 잠금 해제 뒤 실제 활성 창에서 VISUAL-01~06 전체가 통과했다. 최종 통합 검증은 10단계에서 수행한다.

## 변경과 기준

[기준](09-artifacts/baseline/visual-baseline.json)은 1100×720 창에서 72px로 덮인 작업 열, 화면 아래의 상세 영역과 낮은 대비를 기록한다. 프로세스 그리드와 반응형 규칙의 소유자는 process-controls.css로 정리했다. 작업 열은 224px를 확보하고 기본 창에서 상세는 옆에, 좁은 창에서는 위에 표시한다. 클릭·키보드 호출자를 기억하여 닫을 때 해당 대상으로 포커스를 돌린다.

모달 기본 스타일은 components/ui/modal.css 한 곳에 둔다. 초기 입력/설명과 최소한의 안전한 동작에 초점을 맞추고, 제목 연결 및 모달 속성을 명시한다. 중첩 버튼을 담은 행의 button 역할은 그룹으로 변경했다. 로그 탭은 연결된 패널, 선택 상태, 키보드 이동을 제공한다. 검색 포커스, 오류·성공 안내는 공통 규칙을 사용한다.

글자용 primary 색상과 채운 버튼용 action 색상을 분리했다. 라이트 보조 글자도 대비 여유를 확보했다. [W3C 대비 기준](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html)에 따라 활성 일반 글자 4.5:1, 큰 글자 3:1을 검사하며 비활성 입력과 장식은 분리한다. [모달 패턴](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/)의 초기·복귀 포커스를 참고했다.

헤더/사이드바의 중복 상태를 줄이고 테마의 ‘시스템’ 표기를 설정과 통일했다. Panel/PanelHeader/Field/DataTable/TableCell은 src 소비자가 없어 제거하고 실제 사용하는 Button/IconButton/Badge/EmptyState를 유지했다. 프로토타입은 런타임 구성 요소로 간주하지 않았다. 로그 복귀 때 원본 행의 포커스 ID도 보존한다.

## 현재 검증

모든 명령의 cwd는 `/Users/buyonglee/Documents/work/private/my_supervisor`이다. 앱은 `pnpm --dir crates/desktop/ui test:native --spec=visual.spec.mjs --case=VISUAL-02`로 다시 빌드했다. 개별 검증은 `pnpm --dir crates/desktop/ui test:native --no-build --spec=visual.spec.mjs --case=VISUAL-XX`를 사용한다. 실제 revision·source SHA-256·WebView·fixture는 각 manifest/identity에 있다.

| 항목 | 상태와 근거 |
|---|---|
| VISUAL-01 | N-M 통과. 900/1100/1440 너비의 실행·중지·전환 상태와 긴 이름에서 버튼 겹침 없음. 전환 상태만 명시적 표시 fixture 사용. [결과](09-artifacts/actions-native/results.jsonl) |
| VISUAL-02 | N-R 통과. 마지막 프로세스 상세가 보이고 닫으면 실제 호출 버튼으로 복귀. [결과](09-artifacts/detail-native/results.jsonl) |
| VISUAL-03 | N-R 통과. 활성 창에서 `document.hasFocus()=true`, `:focus-within=true`, 검색 외곽선 solid를 확인하고 내부 시작 동작이 기존 상세 선택을 유지했다. [최종 결과](09-artifacts/final-native/results.jsonl) |
| VISUAL-04 | N-R 통과. 두 테마의 화면/흐름/편집 창에서 활성 글자 대비 검사. 색 전환 애니메이션을 끝낸 후 계산 색으로 확인. [결과](09-artifacts/contrast-native/results.jsonl) |
| VISUAL-05 | N-M 통과. 오류 안내는 포커스를 빼앗지 않고 새 모달은 입력에 포커스. 실패 fixture는 listProcessesPage에 한정. [결과](09-artifacts/feedback-native/results.jsonl) |
| VISUAL-06 | N-R 통과. 헤더와 설정의 테마 일치, 기술 정보 유지, 기존 reduced-motion 1ms 규칙 확인. 실제 OS 모션 설정 변경은 하지 않음. [결과](09-artifacts/theme-native/results.jsonl) |

잠금 때문에 실패했던 실행은 [이전 기록](09-artifacts/focus-blocked/native-lock.json)으로 보존한다. 잠금 해제 후 `pnpm --dir crates/desktop/ui test:native --no-build --spec=visual.spec.mjs`로 6개 전체를 다시 실행해 exit 0과 정리 PASS를 확인했다. [계산값·포커스](09-artifacts/final-native/visual-results.json), [앱 식별](09-artifacts/final-native/identity.json), [실행 명세](09-artifacts/final-native/manifest.json)를 함께 보존한다.

## 보존한 검사와 남은 조건

[검사 명령](09-artifacts/checks.json)의 UI typecheck/build, navigation 단위 검사, 기존 parity/event 검증이 통과했다. [로그 회귀 7개](09-artifacts/log-regression/results.jsonl)와 [저장 회귀 6개](09-artifacts/save-regression/results.jsonl)도 통과했다. Vite의 기존 HMR EPERM은 렌더러 단위 검사 결과와 구분한다.

측정에는 활성 HTML 글자, 입력·선택 값, SVG 글자와 해당 라벨 배경을 포함한다. SVG 라벨의 별도 배경을 무시하면 잘못된 대비 실패가 생기므로 배경을 합성한 계산을 썼다. 시각적 WCAG 전체 인증을 주장하지 않는다. 비활성 창에서는 focus pseudo-class가 적용되지 않는 것을 실제로 확인했으므로 포커스 표시를 CSS 강제 주입으로 통과시키지 않았다.

CSS 소유권은 프로세스 열·상세·프로세스 모달의 크기: process-controls.css, 공통 모달: components/ui/modal.css, 테마 토큰: shared/theme.css, 공통 피드백: components/ui/primitives.css로 정리했다. 기본 생성 스키마에 남은 테스트 플러그인 변경은 원복했다. 사용자 변경인 .codemap 상태와 기존 작업은 유지했다. 모든 마지막 native 실행의 앱·소유 프로세스·포트 정리를 확인했다.

10단계 인계: 모든 화면 시나리오는 통과했다. 최종 앱 재빌드 후 기능별 native 회귀와 실제 파일 선택·취소, 클립보드, 창 숨김/트레이 복귀를 검증한다. N-M 전환 상태/오류 fixture와 N-R 결과를 구분한다.
