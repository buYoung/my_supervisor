# [test] Verify the integrated UX in native Tauri

## Work Type
test

## Current State (As-Is)
- [confirmed] Snapshot: commit `748537710413f224be9f25a65d114426b33c0770` on `feat/desktop-parity`, inspected on 2026-10-27 — Evidence: `git rev-parse HEAD`, `git branch --show-current` and the stable source locators below.
- [confirmed] The 2026-10-27 review ran `pnpm --dir crates/desktop/ui typecheck`, `pnpm --dir crates/desktop/ui build`, `node tests/operations-parity.test.mjs` and `pnpm test:follow-events` with exit 0. The last two also logged a Vite WebSocket bind EPERM warning. Those commands did not exercise native file dialogs, IME, actual saves or lifecycle operations.
- [confirmed] The repository's operator-UI evidence records native scale/partial/cursor/keyboard observations as missing or partial; these are not proof that the current views pass.
- [confirmed] `build_host` deliberately prevents a release embedded owner; native debug-test success cannot be relabelled installed-release readiness.

## Desired Outcome (To-Be)
- Produce a complete, attributable result matrix for the requested concerns on the integrated native application.
- Keep unavailable observations explicit and route genuine failures to their owning child before accepting the set.

## Scope
### In Scope
- Run the exact native commands recorded by child 01 against the final integrated revision and compiled frontend, including every CMD/INPUT/SAVE/SCHEDULE/STATE/FLOW/LOG/VISUAL case.
- Observe real save/reload behavior, expected-revision conflicts, exact fixture argv/environment, explicit help probe boundaries, schedule defaults and partial rolling results through actual invoke handlers.
- Verify keyboard-only flow, native resize/contrast, clipboard and actual file selection/cancellation. Mock dialog results may supplement but do not replace native dialog evidence.
- Observe real macOS Korean IME composition/commit behavior. Detect whether the chosen automation actually drives the OS input method; use a recorded native/manual session when it does not.
- Exercise window close-to-tray/reopen with retained application state and controlled owned fixtures; distinguish hiding the window from quitting and from cancelling a running job.
- Exercise existing 250-job/50-instance/10,000-log fixture shapes, failure/partial replies and old-page/cursor boundaries without claiming unmeasured throughput or allocating unnecessary live workloads.
- Verify no mocks remain in the real-IPC lane and that normal non-test artifacts do not include test-only command/listener/permission surfaces.
- Reconcile outdated operator-UI evidence with exact new observations. Preserve historical results with their revision instead of replacing them with an unqualified pass.
- On failure, identify the owning child, pause set acceptance, apply its bounded correction and rerun affected proof plus required integration checks; do not quietly waive or skip a mandatory row.
### Out of Scope
- [hard] Changing the production service-owner/release policy, canonical user state or existing public wire identity contracts as a shortcut for this child.
- [deferred] Signed distribution, installed-owner rollout, login/reboot certification and Windows/Linux support.

## Constraints
- This brief implements the approved desktop UX review and task-related Tauri test scope. Keep user-visible prose in Korean and preserve exact identifiers, logs and user-supplied values.
- Keep `/api/v1`, snake_case DTO keys, camelCase UI mappings, existing invoke command/argument names, definition/instance/run IDs, workflow node/edge/route IDs and expected-config/revision checks compatible.
- Use only task-owned fixtures and temporary data/config/log roots. Never read production credentials or mutate existing user processes/jobs/services for a test.
- Native WebView with mocked IPC, actual invoke/backend evidence and native OS observation are distinct results. A browser/mock pass must not satisfy a required native observation.
- Keep test plugins, frontend imports, execute permissions, fixtures and listeners out of normal builds. Preserve production CSP and the current release-host gate.
- Add only task-relevant tests and necessary development dependencies authorized by the user's Tauri-testing request. Do not introduce unrelated lint/formatter configuration.
- Do not commit or push without a separate user request. If a worktree is needed, use only `/Volumes/Samsung_990_PRO/worktrees/my_supervisor/<name>` while the volume is mounted and do not remove it without asking.
- This is an evidence-producing child. If a proof fails, stop set acceptance, return to the parent, activate the owning correction and re-verification, and recalculate handoffs before resuming.
- Do not install paid drivers, request production credentials, mutate real user jobs or add CI publishing as part of this verification.

## Related Files / Entry Points
- `crates/desktop/ui/tests` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/src/main.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/tauri.conf.json` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/capabilities/main.json` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/services/invoke-client.ts` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/services/http-client.ts` — Read the named behavior in Current State and keep changes within this child's scope.
- `docs/evidence/macos-prod-08-operator-ui.md` — Read the named behavior in Current State and keep changes within this child's scope.
- `docs/evidence/desktop-ux/native-matrix.md` (proposed) — Add only the bounded artifact required by this child.
- `docs/evidence/desktop-ux/10-native-gates.md` (proposed) — Publish the versioned execution and verification handoff.
- `docs/evidence/desktop-ux/09-presentation.md` (proposed) — Consume the preceding contract and actual verification command manifest.

## Execution Plan
### Stage 1 — Establish the bounded baseline
- Starts when: `docs/evidence/desktop-ux/09-presentation.md` is available with prerequisite readiness and its recorded native commands, contracts and remaining limitations; inspect the current checkout before reproducing this child.
- Work: Compare the complete scenario inventory and predecessor evidence with the integrated revision, then enumerate missing native and OS observations. Read applicable package AGENTS.md files before the first edit.
- No-op when: The complete integrated native matrix already exists for the current revision and every mandatory row plus isolation check has actual passing evidence.
- No-op handoff: Publish current-revision proof and `prerequisite_ready: true` at `docs/evidence/desktop-ux/10-native-gates.md` (proposed) for parent global acceptance; the parent may skip edits but must retain required integration gates. A failed proof activates Stage 2 instead of waiving the concern.
- Deliverable: A baseline section at `docs/evidence/desktop-ux/10-native-gates.md` (proposed) with scenario IDs, expected/actual behavior, reproduction mode and any disproved candidate.
- Verify: `Inspect the predecessor handoff and execute its bounded baseline scenarios`; Inputs: this child's named entry points and `docs/evidence/desktop-ux/09-presentation.md`; Expected: Every child scenario has a reproducible baseline or a specific disproof with evidence.
- Ends when:
  - [x] Every in-scope concern is assigned a concrete scenario and evidence mode.
  - [x] The first edit and preserved contracts are identified, or the complete no-change proof is recorded.
- Handoff: Stage 2 receives the baseline, fixture inputs and contract boundaries at `docs/evidence/desktop-ux/10-native-gates.md` (proposed).
- Replan when: Any required proof fails or a native observation cannot be produced. Stop set acceptance, return the failing row to the parent, activate bounded correction/re-verification and update topology/handoffs before resuming; an unavailable result is never a pass.

### Stage 2 — Complete the cohesive change
- Starts when: Stage 1 has recorded the baseline at `docs/evidence/desktop-ux/10-native-gates.md` and no required contract decision is unresolved.
- Work: Execute the established tests and native observations, extend only task-relevant missing scenarios, and collect an auditable final matrix. From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.
- Deliverable: The executed native/OS results plus any bounded test-only coverage additions and exact invocation records, linked from `docs/evidence/desktop-ux/10-native-gates.md` (proposed).
- Verify: `Inspect changed consumers and run the recorded relevant checks`; Inputs: the scoped diff and this child's scenario IDs; Expected: typecheck and applicable checks exit 0 and the changed value/behavior reaches the final consumer.
- Ends when:
  - [x] Every In Scope observation is executed or explicitly identified as unavailable and blocking; product corrections return to their owning child.
  - [x] The evidence compares actual final-consumer state with visible UI state and assigns any discrepancy to a correction owner.
  - [x] Actual commands, exit codes and environment warnings are recorded.
- Handoff: Stage 3 receives the integrated diff, runnable scenarios and result locations from `docs/evidence/desktop-ux/10-native-gates.md` (proposed).
- Replan when: Any required proof fails or a native observation cannot be produced. Stop set acceptance, return the failing row to the parent, activate bounded correction/re-verification and update topology/handoffs before resuming; an unavailable result is never a pass.
- Worker decision: Choose local component/hook names and minimal compatible test dependencies within these contracts; do not widen product behavior or introduce another test framework without a concrete need.

### Stage 3 — Prove the native outcome and hand off
- Starts when: Stage 2 checks and all affected consumer updates are available on the same revision.
- Work: Use the exact cwd/command/scenario selector published in `docs/evidence/desktop-ux/01-test-harness.md`; do not invent a command that has not been implemented. Collect native evidence from the real app and label any mocked lane explicitly. Evaluate every Side Effect Checkpoint and Acceptance Criterion against this saved revision.
- Deliverable: Final `docs/evidence/desktop-ux/10-native-gates.md` (proposed) containing revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits, plus an explicit prerequisite readiness decision.
- Verify: `Inspect the complete scenario report and replay the exact recorded command for the affected native lane`; Inputs: this child's Acceptance Criteria and its native fixture set; Expected: every required row has executed evidence and no missing, mocked-only or skipped native row is reported as a pass.
- Ends when:
  - [x] Each scenario result is attributed to the actual revision, fixture and evidence mode.
  - [x] Isolation/cleanup and cross-consumer side effects are verified, with unavailable observations explicit.
  - [x] The next consumer can locate the exact commands and contract changes without reconstructing this conversation.
- Handoff: parent global acceptance receives `docs/evidence/desktop-ux/10-native-gates.md` (proposed), including the runnable command manifest, verified contracts and remaining limitations.
- Replan when: Any required proof fails or a native observation cannot be produced. Stop set acceptance, return the failing row to the parent, activate bounded correction/re-verification and update topology/handoffs before resuming; an unavailable result is never a pass.

## Side Effect Checkpoints
- [x] Existing HTTP and invoke mapping consumers still observe the same public keys, IDs and error contracts.
- [x] User inputs and inactive drafts are not silently normalized, lost or applied to a different object.
- [x] Existing cancellation, concurrency and revision protections are preserved through the final consumer.
- [x] Native and mocked fixtures remain separated, real user state is untouched and owned test processes/ports are cleaned up.
- [x] From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.

## Acceptance Criteria
- [x] GATE-01: Every mandatory scenario has a non-empty executed result with revision, evidence kind, platform/WebView, command/cwd, fixture, assertions and artifact links.
- [x] GATE-02: Real IPC cases prove saved or final-consumer values rather than only mocked request arguments; all mock contamination checks pass.
- [x] GATE-03: Korean IME, native picker cancellation, focus return, clipboard and close-to-tray observations are recorded from the actual app or explicitly remain unavailable and blocking acceptance.
- [x] GATE-04: Existing typecheck/build/parity/event checks and required Rust checks pass for the final revision with environment warnings accounted for.
- [x] GATE-05: Isolation and teardown pass after both success and a deliberately failed scenario, and ordinary build/config inspection excludes test hooks.
- [x] GATE-06: The final matrix reports remaining release-host/platform limitations separately and never claims shipping-release certification from the development test host.
- [x] `docs/evidence/desktop-ux/10-native-gates.md` records revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits; no remaining required scenario is hidden as an optional follow-up.

## Open Questions
- None — the user selected comprehensive desktop UX improvement and direct Tauri verification; compatibility is preserved, optional expansions are explicitly deferred, and technical choices are bounded above.

## 현재 실행 결과

필수 범위를 완료했다. 기존 자동 58개 중 IME 수정에 영향받는 입력 8개만 재검증했고 통과했다. 나머지 50개와 정리 14회, 기존 폴더·클립보드·트레이 관찰은 영향 분석으로 재사용했다. 실행 파일 선택·취소와 현재 IME·포커스는 격리 VM에서 확인해 GATE-01/03을 닫았다. 실패 기록은 보존하며 통과로 바꾸지 않았다. 사용자 화면·입력·클립보드를 사용하지 않았고 VM은 정지했다. [최종 기록](../evidence/desktop-ux/10-native-gates.md) 참조.
