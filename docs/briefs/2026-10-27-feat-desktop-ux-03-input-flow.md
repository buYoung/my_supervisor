# [feat] Make repeated input and form drafts predictable

## Work Type
feat

## Current State (As-Is)
- [confirmed] Snapshot: commit `748537710413f224be9f25a65d114426b33c0770` on `feat/desktop-parity`, inspected on 2026-10-27 — Evidence: `git rev-parse HEAD`, `git branch --show-current` and the stable source locators below.
- [confirmed] `JobEditor` normalizes comma-separated dependencies on each change and `RestartGuards` removes blank exclusion lines immediately; browser review reproduced merged tokens instead of a new item.
- [confirmed] `EnvironmentEditor` filters/sorts directly from saved-shaped keys, adds NEW_VARIABLE behind the active search, and silently restores invalid/duplicate key edits on blur.
- [confirmed] `ProcessEditor` and `StepSettings` convert number input with `Number()` on every keystroke; clearing the instance field visibly produces 0.
- [confirmed] `CheckControl`, guard toggles and `JobEditor::changeTrigger` recreate defaults when switching off/on or between types; the reviewed URL and 120-second schedule drafts were lost.
- [confirmed] `ProcessSpec::validate_production_policy` rejects follow_symlinks=true, zero debounce, invalid capacity and check timeout >= interval. `should_include_path` compares path prefixes, not globs.

## Desired Outcome (To-Be)
- Let users continue multi-item entry on the next visible row while retaining their unfinished text.
- Keep validation, focus and inactive policy drafts consistent with actual backend capabilities.

## Scope
### In Scope
- Implement line-per-item dependency entry with searchable registered-job selection. A comma or Enter commits an unambiguous item and moves to the next row; provide an explicit Add action with the same result.
- Preserve names containing commas through whole-name selection. Do not change accepted process/job names or silently split an ambiguous existing name.
- Handle empty delimiters, duplicate selections, unknown jobs, self-dependencies, multiple-item paste, editing a middle item and deletion without dropping text or reordering existing entries.
- Treat paths as literal per-row values with Enter/Add and path pickers; preserve commas/spaces. Make bulk import explicit and show the parsed rows before accepting an ambiguous split.
- Keep raw numeric drafts until validation/commit; distinguish empty, zero, omitted/default and unlimited values. Retain existing wire units and numeric precision.
- Keep per-type/per-toggle drafts within the open editor. Serialize only the selected trigger and enabled policies, so remembering a disabled value does not enable it.
- Keep a newly added environment row visible during search, focus its name, show invalid/duplicate-key errors beside the field, and preserve both draft key and value.
- Show environment import add/change/delete/conflict counts and distinguish applying to the draft from saving the process/job; preserve a stable original baseline in workflow steps.
- Reflect actual policy constraints before save: unsupported symlink traversal, required absolute watch roots, positive debounce, check timeout below interval, rolling readiness/capacity/routability preconditions.
- Explain exact path-prefix exclusion semantics; do not present relative names or globs as supported matching.
- Support IME-safe item commitment, predictable Tab/Shift-Tab and local row undo; adding/removing draft rows must not require repeated confirmation dialogs.
- Keep long or multiline environment values intact through import and editing; expose an expanded value editor when a single-line field cannot represent the original value.
### Out of Scope
- [hard] Changing the production service-owner/release policy, canonical user state or existing public wire identity contracts as a shortcut for this child.
- [deferred] Persistent drafts across app restarts, glob matching, a new dotenv dialect and arbitrary path expansion.

## Constraints
- This brief implements the approved desktop UX review and task-related Tauri test scope. Keep user-visible prose in Korean and preserve exact identifiers, logs and user-supplied values.
- Keep `/api/v1`, snake_case DTO keys, camelCase UI mappings, existing invoke command/argument names, definition/instance/run IDs, workflow node/edge/route IDs and expected-config/revision checks compatible.
- Use only task-owned fixtures and temporary data/config/log roots. Never read production credentials or mutate existing user processes/jobs/services for a test.
- Native WebView with mocked IPC, actual invoke/backend evidence and native OS observation are distinct results. A browser/mock pass must not satisfy a required native observation.
- Keep test plugins, frontend imports, execute permissions, fixtures and listeners out of normal builds. Preserve production CSP and the current release-host gate.
- Add only task-relevant tests and necessary development dependencies authorized by the user's Tauri-testing request. Do not introduce unrelated lint/formatter configuration.
- Do not commit or push without a separate user request. If a worktree is needed, use only `/Volumes/Samsung_990_PRO/worktrees/my_supervisor/<name>` while the volume is mounted and do not remove it without asking.
- A textarea and a chip list are not interchangeable for all fields; choose row-based editing for long names/paths and preserve literal command/environment contents.
- Do not persist environment-bearing drafts to localStorage as an incidental UX change.

## Related Files / Entry Points
- `crates/desktop/ui/src/features/jobs/JobEditor.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/processes/EnvironmentEditor.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/processes/GuardControls.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/processes/RestartControls.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/processes/ProcessEditor.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/jobs/StepSettings.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/components/ui/PathField.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/core/src/domain/process.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/platform/macos/src/guards.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/components/ui/RepeatedField.tsx` (proposed) — Add only the bounded artifact required by this child.
- `docs/evidence/desktop-ux/03-input-flow.md` (proposed) — Publish the versioned execution and verification handoff.
- `docs/evidence/desktop-ux/02-command-edit.md` (proposed) — Consume the preceding contract and actual verification command manifest.

## Execution Plan
### Stage 1 — Establish the bounded baseline
- Starts when: `docs/evidence/desktop-ux/02-command-edit.md` is available with prerequisite readiness and its recorded native commands, contracts and remaining limitations; inspect the current checkout before reproducing this child.
- Work: Reproduce comma/newline collapse, hidden added environment rows, silent name rollback, numeric empty-to-zero and type/toggle reset cases using native fixtures. Read applicable package AGENTS.md files before the first edit.
- No-op when: All Desired Outcome and Acceptance Criteria already hold on the current revision with the required native and final-consumer evidence.
- No-op handoff: Publish current-revision proof and `prerequisite_ready: true` at `docs/evidence/desktop-ux/03-input-flow.md` (proposed) for docs/briefs/2026-10-27-fix-desktop-ux-04-save-lifecycle.md; the parent may skip edits but must retain required integration gates. A failed proof activates Stage 2 instead of waiving the concern.
- Deliverable: A baseline section at `docs/evidence/desktop-ux/03-input-flow.md` (proposed) with scenario IDs, expected/actual behavior, reproduction mode and any disproved candidate.
- Verify: `Inspect the predecessor handoff and execute its bounded baseline scenarios`; Inputs: this child's named entry points and `docs/evidence/desktop-ux/02-command-edit.md`; Expected: Every child scenario has a reproducible baseline or a specific disproof with evidence.
- Ends when:
  - [x] Every in-scope concern is assigned a concrete scenario and evidence mode.
  - [x] The first edit and preserved contracts are identified, or the complete no-change proof is recorded.
- Handoff: Stage 2 receives the baseline, fixture inputs and contract boundaries at `docs/evidence/desktop-ux/03-input-flow.md` (proposed).
- Replan when: The comma shortcut would require rejecting currently valid names, or preserving drafts requires a wire/schema change. Keep literal selection available and return the proposed contract change to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

### Stage 2 — Complete the cohesive change
- Starts when: Stage 1 has recorded the baseline at `docs/evidence/desktop-ux/03-input-flow.md` and no required contract decision is unresolved.
- Work: Implement shared repeated-field/draft mechanics and integrate the dependent fields atomically while preserving the wire serializer. From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.
- Deliverable: The scoped implementation plus its case-specific assertions and exact invocation records, linked from `docs/evidence/desktop-ux/03-input-flow.md` (proposed).
- Verify: `Inspect changed consumers and run the recorded relevant checks`; Inputs: the scoped diff and this child's scenario IDs; Expected: typecheck and applicable checks exit 0 and the changed value/behavior reaches the final consumer.
- Ends when:
  - [x] All In Scope behavior is implemented or specifically proved already satisfied.
  - [x] Shared callers consume the same contract and failures preserve user intent and valid prior state.
  - [x] Actual commands, exit codes and environment warnings are recorded.
- Handoff: Stage 3 receives the integrated diff, runnable scenarios and result locations from `docs/evidence/desktop-ux/03-input-flow.md` (proposed).
- Replan when: The comma shortcut would require rejecting currently valid names, or preserving drafts requires a wire/schema change. Keep literal selection available and return the proposed contract change to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.
- Worker decision: Choose local component/hook names and minimal compatible test dependencies within these contracts; do not widen product behavior or introduce another test framework without a concrete need.

### Stage 3 — Prove the native outcome and hand off
- Starts when: Stage 2 checks and all affected consumer updates are available on the same revision.
- Work: Use the exact cwd/command/scenario selector published in `docs/evidence/desktop-ux/01-test-harness.md`; do not invent a command that has not been implemented. Collect native evidence from the real app and label any mocked lane explicitly. Evaluate every Side Effect Checkpoint and Acceptance Criterion against this saved revision.
- Deliverable: Final `docs/evidence/desktop-ux/03-input-flow.md` (proposed) containing revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits, plus an explicit prerequisite readiness decision.
- Verify: `Inspect the complete scenario report and replay the exact recorded command for the affected native lane`; Inputs: this child's Acceptance Criteria and its native fixture set; Expected: every required row has executed evidence and no missing, mocked-only or skipped native row is reported as a pass.
- Ends when:
  - [x] Each scenario result is attributed to the actual revision, fixture and evidence mode.
  - [x] Isolation/cleanup and cross-consumer side effects are verified, with unavailable observations explicit.
  - [x] The next consumer can locate the exact commands and contract changes without reconstructing this conversation.
- Handoff: docs/briefs/2026-10-27-fix-desktop-ux-04-save-lifecycle.md receives `docs/evidence/desktop-ux/03-input-flow.md` (proposed), including the runnable command manifest, verified contracts and remaining limitations.
- Replan when: The comma shortcut would require rejecting currently valid names, or preserving drafts requires a wire/schema change. Keep literal selection available and return the proposed contract change to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

## Side Effect Checkpoints
- [x] Existing HTTP and invoke mapping consumers still observe the same public keys, IDs and error contracts.
- [x] User inputs and inactive drafts are not silently normalized, lost or applied to a different object.
- [x] Existing cancellation, concurrency and revision protections are preserved through the final consumer.
- [x] Native and mocked fixtures remain separated, real user state is untouched and owned test processes/ports are cleaned up.
- [x] From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.

## Acceptance Criteria
- [x] INPUT-01: Enter first comma second into dependency entry and obtain two visible selected rows with focus on the next row and two exact names in the saved trigger.
- [x] INPUT-02: Empty delimiter, duplicate/self/unknown item, middle-row edit, Unicode/comma-bearing name and multi-item paste all preserve text and show the correct per-row state.
- [x] INPUT-03: A path containing a comma remains one path; Enter adds a second path. Relative/glob exclusions are explained or rejected rather than silently claimed to match.
- [x] INPUT-04: Clearing a number stays blank while editing; valid 5000ms interval and 3000ms timeout reach saved config and the consumer unchanged, while timeout >= interval is explained before save.
- [x] INPUT-05: Toggle HTTP off/on, HTTP/TCP/HTTP and interval/cron/interval and recover the entered URL and 120-second value within the editor. Disabled alternatives remain absent from the saved policy.
- [x] INPUT-06: Add during DATABASE search, rename to an invalid or duplicate key, delete/undo and import a conflicting environment file; the target row remains visible and no key/value disappears silently.
- [x] INPUT-07: Composition completion does not add a row or submit the form; record native Korean IME observation separately from synthetic event tests.
- [x] INPUT-08: Unsupported symlink traversal cannot be submitted as a seemingly valid option; rolling prerequisites link to the relevant fields.
- [x] `docs/evidence/desktop-ux/03-input-flow.md` records revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits; no remaining required scenario is hidden as an optional follow-up.

## Open Questions
- None — the user selected comprehensive desktop UX improvement and direct Tauri verification; compatibility is preserved, optional expansions are explicitly deferred, and technical choices are bounded above.

## 제한된 최종 검증 상태

현재 제품 코드의 INPUT-07과 지연 blur 이후 포커스를 격리 macOS VM에서 확인했다. VM에서 발견한 조합 확정 Enter 경로를 수정하고 관련 입력 자동 8개만 재검증해 통과했다. 합성 재생과 실제 OS 관찰은 분리했다. 사용자 화면·입력·클립보드를 사용하지 않았고 VM은 정지했다. [최종 판정](../evidence/desktop-ux/10-native-gates.md)을 따른다.
