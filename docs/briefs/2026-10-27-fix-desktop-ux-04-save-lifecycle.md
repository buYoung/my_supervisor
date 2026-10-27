# [fix] Preserve edits through save and navigation

## Work Type
fix

## Current State (As-Is)
- [confirmed] Snapshot: commit `748537710413f224be9f25a65d114426b33c0770` on `feat/desktop-parity`, inspected on 2026-10-27 — Evidence: `git rev-parse HEAD`, `git branch --show-current` and the stable source locators below.
- [confirmed] `Modal` closes on Escape/backdrop and delegates directly to callers; ProcessEditor/JobEditor do not guard unsaved changes.
- [confirmed] `WorkflowWorkspace` guards only its own Back button; `App::navigate` can unmount it without a dirty check.
- [confirmed] `record_job_settings` writes a new workflow revision, while the open workflow loader depends on job.name and retains the previous document.revision.
- [confirmed] `WorkflowWorkspace::save` replaces flow with the response even though canvas and step settings can still be changed during the request.
- [confirmed] Process config updates already carry `expected`, and workflow saves carry `expected_revision`; preserve these existing concurrency contracts.

## Reproduction
- Edit a process then close with Escape/backdrop; the draft is lost without a choice.
- Open a workflow, save Job settings, then edit/save the flow; the old revision is submitted and conflicts.
- Delay a flow save, edit again before its reply and observe whether the reply replaces the new edit.
- Use baseline 748537710413f224be9f25a65d114426b33c0770 and the predecessor's isolated fixtures. Browser observations were repeatable where identified; confirm native frequency and backend-only cases in Stage 1 before changing their implementation.

## Desired Outcome (To-Be)
- Preserve drafts across cancellation, failed saves and in-app navigation, with explicit discard only when needed.
- Keep saved revisions, submitted snapshots and visible edits synchronized without overwriting newer work.

## Scope
### In Scope
- Create a bounded shared edit-session contract for dirty/valid/saving/error/saved state and connect command/input validity from children 02-03.
- Guard Escape, close button, backdrop, cancel, sidebar, Back and ordinary in-app links consistently. Clean views close immediately; dirty views offer Continue editing and Discard.
- Keep drafts in memory within the session and preserve search/selection return context. External reload/close recovery must report its actual supported behavior rather than promise persistence.
- Capture a submitted snapshot and either lock affected editing or retain subsequent edits as dirty; a late save response must not erase new input.
- After JobEditor saves settings, reconcile the new workflow revision and definition into the still-open workspace without silently discarding local flow edits.
- Provide an explicit conflict recovery path using current saved state and retained local changes; never blindly resubmit with a new expected revision.
- Use action labels matching effects: draft import reflection, setting save, live-process apply/restart, registration/start. Show target and changed settings without an extra confirmation at every keystroke.
- Scope save failures and success messages to the current editor/object. Keep close/cancel semantics clear during in-flight requests and prevent duplicate submissions.
### Out of Scope
- [hard] Changing the production service-owner/release policy, canonical user state or existing public wire identity contracts as a shortcut for this child.
- [deferred] Disk-persisted draft recovery after application restart and undoing already executed external process operations.

## Constraints
- This brief implements the approved desktop UX review and task-related Tauri test scope. Keep user-visible prose in Korean and preserve exact identifiers, logs and user-supplied values.
- Keep `/api/v1`, snake_case DTO keys, camelCase UI mappings, existing invoke command/argument names, definition/instance/run IDs, workflow node/edge/route IDs and expected-config/revision checks compatible.
- Use only task-owned fixtures and temporary data/config/log roots. Never read production credentials or mutate existing user processes/jobs/services for a test.
- Native WebView with mocked IPC, actual invoke/backend evidence and native OS observation are distinct results. A browser/mock pass must not satisfy a required native observation.
- Keep test plugins, frontend imports, execute permissions, fixtures and listeners out of normal builds. Preserve production CSP and the current release-host gate.
- Add only task-relevant tests and necessary development dependencies authorized by the user's Tauri-testing request. Do not introduce unrelated lint/formatter configuration.
- Do not commit or push without a separate user request. If a worktree is needed, use only `/Volumes/Samsung_990_PRO/worktrees/my_supervisor/<name>` while the volume is mounted and do not remove it without asking.

## Related Files / Entry Points
- `crates/desktop/ui/src/App.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/components/ui/Modal.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/processes/ProcessEditor.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/processes/ProcessesView.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/jobs/JobEditor.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/jobs/JobsView.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/jobs/WorkflowEditor.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/application/src/workflow.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/infra/sqlite/src/workspace.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/shared/editor-session.ts` (proposed) — Add only the bounded artifact required by this child.
- `docs/evidence/desktop-ux/04-save-lifecycle.md` (proposed) — Publish the versioned execution and verification handoff.
- `docs/evidence/desktop-ux/03-input-flow.md` (proposed) — Consume the preceding contract and actual verification command manifest.

## Execution Plan
### Stage 1 — Establish the bounded baseline
- Starts when: `docs/evidence/desktop-ux/03-input-flow.md` is available with prerequisite readiness and its recorded native commands, contracts and remaining limitations; inspect the current checkout before reproducing this child.
- Work: Record all close/navigation routes and the settings-to-workflow revision transition with controlled response ordering. Read applicable package AGENTS.md files before the first edit.
- No-op when: All Desired Outcome and Acceptance Criteria already hold on the current revision with the required native and final-consumer evidence.
- No-op handoff: Publish current-revision proof and `prerequisite_ready: true` at `docs/evidence/desktop-ux/04-save-lifecycle.md` (proposed) for docs/briefs/2026-10-27-fix-desktop-ux-05-scheduling.md; the parent may skip edits but must retain required integration gates. A failed proof activates Stage 2 instead of waiving the concern.
- Deliverable: A baseline section at `docs/evidence/desktop-ux/04-save-lifecycle.md` (proposed) with scenario IDs, expected/actual behavior, reproduction mode and any disproved candidate.
- Verify: `Inspect the predecessor handoff and execute its bounded baseline scenarios`; Inputs: this child's named entry points and `docs/evidence/desktop-ux/03-input-flow.md`; Expected: Every child scenario has a reproducible baseline or a specific disproof with evidence.
- Ends when:
  - [x] Every in-scope concern is assigned a concrete scenario and evidence mode.
  - [x] The first edit and preserved contracts are identified, or the complete no-change proof is recorded.
- Handoff: Stage 2 receives the baseline, fixture inputs and contract boundaries at `docs/evidence/desktop-ux/04-save-lifecycle.md` (proposed).
- Replan when: Conflict resolution would overwrite an external edit or would require changing the existing revision/expected-config contract. Stop automatic reconciliation and return the concrete case to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

### Stage 2 — Complete the cohesive change
- Starts when: Stage 1 has recorded the baseline at `docs/evidence/desktop-ux/04-save-lifecycle.md` and no required contract decision is unresolved.
- Work: Integrate the shared edit lifecycle through all editor consumers while preserving existing expected-value checks. From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.
- Deliverable: The scoped implementation plus its case-specific assertions and exact invocation records, linked from `docs/evidence/desktop-ux/04-save-lifecycle.md` (proposed).
- Verify: `Inspect changed consumers and run the recorded relevant checks`; Inputs: the scoped diff and this child's scenario IDs; Expected: typecheck and applicable checks exit 0 and the changed value/behavior reaches the final consumer.
- Ends when:
  - [x] All In Scope behavior is implemented or specifically proved already satisfied.
  - [x] Shared callers consume the same contract and failures preserve user intent and valid prior state.
  - [x] Actual commands, exit codes and environment warnings are recorded.
- Handoff: Stage 3 receives the integrated diff, runnable scenarios and result locations from `docs/evidence/desktop-ux/04-save-lifecycle.md` (proposed).
- Replan when: Conflict resolution would overwrite an external edit or would require changing the existing revision/expected-config contract. Stop automatic reconciliation and return the concrete case to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.
- Worker decision: Choose local component/hook names and minimal compatible test dependencies within these contracts; do not widen product behavior or introduce another test framework without a concrete need.

### Stage 3 — Prove the native outcome and hand off
- Starts when: Stage 2 checks and all affected consumer updates are available on the same revision.
- Work: Use the exact cwd/command/scenario selector published in `docs/evidence/desktop-ux/01-test-harness.md`; do not invent a command that has not been implemented. Collect native evidence from the real app and label any mocked lane explicitly. Evaluate every Side Effect Checkpoint and Acceptance Criterion against this saved revision.
- Deliverable: Final `docs/evidence/desktop-ux/04-save-lifecycle.md` (proposed) containing revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits, plus an explicit prerequisite readiness decision.
- Verify: `Inspect the complete scenario report and replay the exact recorded command for the affected native lane`; Inputs: this child's Acceptance Criteria and its native fixture set; Expected: every required row has executed evidence and no missing, mocked-only or skipped native row is reported as a pass.
- Ends when:
  - [x] Each scenario result is attributed to the actual revision, fixture and evidence mode.
  - [x] Isolation/cleanup and cross-consumer side effects are verified, with unavailable observations explicit.
  - [x] The next consumer can locate the exact commands and contract changes without reconstructing this conversation.
- Handoff: docs/briefs/2026-10-27-fix-desktop-ux-05-scheduling.md receives `docs/evidence/desktop-ux/04-save-lifecycle.md` (proposed), including the runnable command manifest, verified contracts and remaining limitations.
- Replan when: Conflict resolution would overwrite an external edit or would require changing the existing revision/expected-config contract. Stop automatic reconciliation and return the concrete case to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

## Side Effect Checkpoints
- [x] Existing HTTP and invoke mapping consumers still observe the same public keys, IDs and error contracts.
- [x] User inputs and inactive drafts are not silently normalized, lost or applied to a different object.
- [x] Existing cancellation, concurrency and revision protections are preserved through the final consumer.
- [x] Native and mocked fixtures remain separated, real user state is untouched and owned test processes/ports are cleaned up.
- [x] From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.

## Acceptance Criteria
- [x] SAVE-01: Each close/navigation route preserves dirty work unless Discard is explicitly chosen; a clean editor closes without an extra prompt.
- [x] SAVE-02: Invalid command/numeric/dependency drafts cannot submit a previous valid snapshot.
- [x] SAVE-03: Saving Job settings then saving an edited flow succeeds with the correct revision, or presents a recoverable real concurrent conflict while retaining the draft.
- [x] SAVE-04: Delayed success/failure and edits made during save never overwrite newer input or falsely mark it saved.
- [x] SAVE-05: Retry, double click and cancelled navigation cause the intended number of backend mutations; no unexpected process restart occurs.
- [x] SAVE-06: Environment import says it updated the draft until the outer save completes; live process updates communicate the actual restart effect.
- [x] `docs/evidence/desktop-ux/04-save-lifecycle.md` records revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits; no remaining required scenario is hidden as an optional follow-up.

## Open Questions
- None — the user selected comprehensive desktop UX improvement and direct Tauri verification; compatibility is preserved, optional expansions are explicitly deferred, and technical choices are bounded above.
