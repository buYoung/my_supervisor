# [feat] Improve workflow editing and history continuity

## Work Type
feat

## Current State (As-Is)
- [confirmed] Snapshot: commit `748537710413f224be9f25a65d114426b33c0770` on `feat/desktop-parity`, inspected on 2026-10-27 — Evidence: `git rev-parse HEAD`, `git branch --show-current` and the stable source locators below.
- [confirmed] `StepSettings` states that result routes use first-match order, but provides add/delete controls without a reorder operation.
- [confirmed] `WorkflowWorkspace` calls fitView when palette/mode/run/version/node-count values change and has no draft undo/redo state.
- [confirmed] `execute` submits the current definition, while a job-call step references another saved job and importJob copies its nodes; these are different behaviors requiring distinct labels.
- [confirmed] Workflow revision saves and settings reconciliation must consume child 04's edit-session contract.

## Desired Outcome (To-Be)
- Make local graph edits reversible and keep the user's viewport and selection stable.
- Communicate draft execution, copied steps, referenced jobs and historical versions without changing execution semantics.

## Scope
### In Scope
- Add local draft undo/redo for node/edge creation, removal, connection changes, settings edits and result-route reordering; keep stable node/edge/route IDs.
- Provide keyboard-accessible result-route ordering and connection editing. Preserve first-match routing and existing cycle/one-output validation.
- Retain pan/zoom across palette toggles, inspection and ordinary edits; fit explicitly or on first load when needed, and make newly added nodes discoverable without resetting the entire view.
- Distinguish job scheduling dependencies from inner workflow edges in headings/help. Label copy/import and saved-job reference actions according to their actual behavior.
- Make execution scope explicit: current draft, selected step only, or selected job only. Do not auto-save or rewrite downstream execution semantics as a convenience.
- Keep edit, execution history and version comparison visual states separate, with meaningful settings labels and preserved access to raw values.
- Group optional retry/circuit/routing controls progressively while showing a compact summary when configured; do not hide active policies.
- Keep graph selection and route changes compatible with the shared save/conflict handling and form draft mechanics.
### Out of Scope
- [hard] Changing the production service-owner/release policy, canonical user state or existing public wire identity contracts as a shortcut for this child.
- [deferred] Automatic layout redesign, a new workflow engine and compensation/undo of real executed actions.

## Constraints
- This brief implements the approved desktop UX review and task-related Tauri test scope. Keep user-visible prose in Korean and preserve exact identifiers, logs and user-supplied values.
- Keep `/api/v1`, snake_case DTO keys, camelCase UI mappings, existing invoke command/argument names, definition/instance/run IDs, workflow node/edge/route IDs and expected-config/revision checks compatible.
- Use only task-owned fixtures and temporary data/config/log roots. Never read production credentials or mutate existing user processes/jobs/services for a test.
- Native WebView with mocked IPC, actual invoke/backend evidence and native OS observation are distinct results. A browser/mock pass must not satisfy a required native observation.
- Keep test plugins, frontend imports, execute permissions, fixtures and listeners out of normal builds. Preserve production CSP and the current release-host gate.
- Add only task-relevant tests and necessary development dependencies authorized by the user's Tauri-testing request. Do not introduce unrelated lint/formatter configuration.
- Do not commit or push without a separate user request. If a worktree is needed, use only `/Volumes/Samsung_990_PRO/worktrees/my_supervisor/<name>` while the volume is mounted and do not remove it without asking.
- Undo/redo only changes the local draft; it must never imply reversal of an already executed process/job side effect.

## Related Files / Entry Points
- `crates/desktop/ui/src/features/jobs/WorkflowEditor.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/jobs/StepSettings.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/jobs/workflow-model.ts` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/jobs/JobsView.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/services/workspace-types.ts` — Read the named behavior in Current State and keep changes within this child's scope.
- `docs/evidence/desktop-ux/07-workflow-edit.md` (proposed) — Publish the versioned execution and verification handoff.
- `docs/evidence/desktop-ux/06-operator-state.md` (proposed) — Consume the preceding contract and actual verification command manifest.

## Execution Plan
### Stage 1 — Establish the bounded baseline
- Starts when: `docs/evidence/desktop-ux/06-operator-state.md` is available with prerequisite readiness and its recorded native commands, contracts and remaining limitations; inspect the current checkout before reproducing this child.
- Work: Observe result-route priority editing, delete/reconnect recovery, viewport jumps and the identity of the definition executed from a dirty workspace. Read applicable package AGENTS.md files before the first edit.
- No-op when: All Desired Outcome and Acceptance Criteria already hold on the current revision with the required native and final-consumer evidence.
- No-op handoff: Publish current-revision proof and `prerequisite_ready: true` at `docs/evidence/desktop-ux/07-workflow-edit.md` (proposed) for docs/briefs/2026-10-27-feat-desktop-ux-08-log-navigation.md; the parent may skip edits but must retain required integration gates. A failed proof activates Stage 2 instead of waiving the concern.
- Deliverable: A baseline section at `docs/evidence/desktop-ux/07-workflow-edit.md` (proposed) with scenario IDs, expected/actual behavior, reproduction mode and any disproved candidate.
- Verify: `Inspect the predecessor handoff and execute its bounded baseline scenarios`; Inputs: this child's named entry points and `docs/evidence/desktop-ux/06-operator-state.md`; Expected: Every child scenario has a reproducible baseline or a specific disproof with evidence.
- Ends when:
  - [x] Every in-scope concern is assigned a concrete scenario and evidence mode.
  - [x] The first edit and preserved contracts are identified, or the complete no-change proof is recorded.
- Handoff: Stage 2 receives the baseline, fixture inputs and contract boundaries at `docs/evidence/desktop-ux/07-workflow-edit.md` (proposed).
- Replan when: Graph history or reconnection requires changing workflow IDs, result-route semantics or persisted version format. Stop and return that compatibility change to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

### Stage 2 — Complete the cohesive change
- Starts when: Stage 1 has recorded the baseline at `docs/evidence/desktop-ux/07-workflow-edit.md` and no required contract decision is unresolved.
- Work: Implement bounded graph history and interaction affordances over existing workflow models, preserving serialization and execution policies. From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.
- Deliverable: The scoped implementation plus its case-specific assertions and exact invocation records, linked from `docs/evidence/desktop-ux/07-workflow-edit.md` (proposed).
- Verify: `Inspect changed consumers and run the recorded relevant checks`; Inputs: the scoped diff and this child's scenario IDs; Expected: typecheck and applicable checks exit 0 and the changed value/behavior reaches the final consumer.
- Ends when:
  - [x] All In Scope behavior is implemented or specifically proved already satisfied.
  - [x] Shared callers consume the same contract and failures preserve user intent and valid prior state.
  - [x] Actual commands, exit codes and environment warnings are recorded.
- Handoff: Stage 3 receives the integrated diff, runnable scenarios and result locations from `docs/evidence/desktop-ux/07-workflow-edit.md` (proposed).
- Replan when: Graph history or reconnection requires changing workflow IDs, result-route semantics or persisted version format. Stop and return that compatibility change to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.
- Worker decision: Choose local component/hook names and minimal compatible test dependencies within these contracts; do not widen product behavior or introduce another test framework without a concrete need.

### Stage 3 — Prove the native outcome and hand off
- Starts when: Stage 2 checks and all affected consumer updates are available on the same revision.
- Work: Use the exact cwd/command/scenario selector published in `docs/evidence/desktop-ux/01-test-harness.md`; do not invent a command that has not been implemented. Collect native evidence from the real app and label any mocked lane explicitly. Evaluate every Side Effect Checkpoint and Acceptance Criterion against this saved revision.
- Deliverable: Final `docs/evidence/desktop-ux/07-workflow-edit.md` (proposed) containing revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits, plus an explicit prerequisite readiness decision.
- Verify: `Inspect the complete scenario report and replay the exact recorded command for the affected native lane`; Inputs: this child's Acceptance Criteria and its native fixture set; Expected: every required row has executed evidence and no missing, mocked-only or skipped native row is reported as a pass.
- Ends when:
  - [x] Each scenario result is attributed to the actual revision, fixture and evidence mode.
  - [x] Isolation/cleanup and cross-consumer side effects are verified, with unavailable observations explicit.
  - [x] The next consumer can locate the exact commands and contract changes without reconstructing this conversation.
- Handoff: docs/briefs/2026-10-27-feat-desktop-ux-08-log-navigation.md receives `docs/evidence/desktop-ux/07-workflow-edit.md` (proposed), including the runnable command manifest, verified contracts and remaining limitations.
- Replan when: Graph history or reconnection requires changing workflow IDs, result-route semantics or persisted version format. Stop and return that compatibility change to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

## Side Effect Checkpoints
- [x] Existing HTTP and invoke mapping consumers still observe the same public keys, IDs and error contracts.
- [x] User inputs and inactive drafts are not silently normalized, lost or applied to a different object.
- [x] Existing cancellation, concurrency and revision protections are preserved through the final consumer.
- [x] Native and mocked fixtures remain separated, real user state is untouched and owned test processes/ports are cleaned up.
- [x] From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.

## Acceptance Criteria
- [x] FLOW-01: Add/delete/reconnect/change/reorder then undo/redo restores the exact intended graph without duplicating IDs or losing connected route handles.
- [x] FLOW-02: Reorder two matching result routes and verify the intended first route is chosen by a real isolated workflow run.
- [x] FLOW-03: Palette toggles and node inspection preserve a deliberately chosen pan/zoom; explicit fit still works.
- [x] FLOW-04: Copying steps creates independent nodes, referencing a job uses its saved flow, and labels make the difference clear.
- [x] FLOW-05: Running a dirty draft or one selected step executes only the declared scope and records the correct execution snapshot without implicitly saving it.
- [x] FLOW-06: Switching between edit/run/version views does not leak historical status or diff styling into the editable graph.
- [x] FLOW-07: Graph operations and route ordering are achievable by keyboard, with stable focus and clear validation errors.
- [x] `docs/evidence/desktop-ux/07-workflow-edit.md` records revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits; no remaining required scenario is hidden as an optional follow-up.

## Open Questions
- None — the user selected comprehensive desktop UX improvement and direct Tauri verification; compatibility is preserved, optional expansions are explicitly deferred, and technical choices are bounded above.
