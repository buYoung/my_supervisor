# [feat] Preserve log context and readable results

## Work Type
feat

## Current State (As-Is)
- [confirmed] Snapshot: commit `748537710413f224be9f25a65d114426b33c0770` on `feat/desktop-parity`, inspected on 2026-10-27 — Evidence: `git rev-parse HEAD`, `git branch --show-current` and the stable source locators below.
- [confirmed] `LogsView` defaults to attention levels and a seven-day range even when entered through links labelled 전체 로그.
- [confirmed] `LogRow` shows only time in the row and puts the date in a tooltip, although queries can span many days.
- [confirmed] `isLive` schedules a five-second poll and suppresses refresh during context view, non-first pages or loading; its visible state does not describe all these cases.
- [confirmed] Process detail and log explorer implement different raw-log dialogs; the richer inspector provides metadata, copy and surrounding context.
- [confirmed] `log_explorer::query` sets snapshot_at from request.until when supplied; custom range export widening was investigated and is not an established defect.

## Desired Outcome (To-Be)
- Make active filters, dates, refresh state and incomplete data immediately understandable.
- Keep source/run/step context and reading position stable through detail navigation, refresh and export.

## Scope
### In Scope
- Align whole-log entry links with actual filter intent; show active filters and a clear reset/all-level action when they explain an empty result.
- Retain source/run/step/search/range/order/grouping and selected position through inspector/context/Back navigation without leaking environment values into URLs.
- Show dates or date separators for multi-day results and preserve timezone consistency with the display-time provider.
- Describe the actual automatic refresh interval and paused reasons. Keep currently inspected content stable and offer a new-record indicator instead of silently moving the reading position.
- Share a raw-log inspector across preview and explorer with metadata, copy feedback and surrounding-context navigation; distinguish current raw content from formatted display.
- Keep partial-source, truncation and error states explicit. Preserve snapshot and pagination boundaries during group expansion and export.
- Reduce always-visible explanatory/filter controls using clear presets and progressive disclosure without hiding active restrictions.
- Keep export progress/failure feedback accurate and avoid an unexplained success state after incomplete retrieval. Bound rendering/working buffers without silently truncating the requested export; retain selected-snapshot completeness.
- Keep copy/export notices inside the active inspector or visible result area so modal focus does not hide feedback behind the dialog.
### Out of Scope
- [hard] Changing the production service-owner/release policy, canonical user state or existing public wire identity contracts as a shortcut for this child.
- [deferred] New retention policies, log ingestion formats and a separate analytics dashboard.

## Constraints
- This brief implements the approved desktop UX review and task-related Tauri test scope. Keep user-visible prose in Korean and preserve exact identifiers, logs and user-supplied values.
- Keep `/api/v1`, snake_case DTO keys, camelCase UI mappings, existing invoke command/argument names, definition/instance/run IDs, workflow node/edge/route IDs and expected-config/revision checks compatible.
- Use only task-owned fixtures and temporary data/config/log roots. Never read production credentials or mutate existing user processes/jobs/services for a test.
- Native WebView with mocked IPC, actual invoke/backend evidence and native OS observation are distinct results. A browser/mock pass must not satisfy a required native observation.
- Keep test plugins, frontend imports, execute permissions, fixtures and listeners out of normal builds. Preserve production CSP and the current release-host gate.
- Add only task-relevant tests and necessary development dependencies authorized by the user's Tauri-testing request. Do not introduce unrelated lint/formatter configuration.
- Do not commit or push without a separate user request. If a worktree is needed, use only `/Volumes/Samsung_990_PRO/worktrees/my_supervisor/<name>` while the volume is mounted and do not remove it without asking.

## Related Files / Entry Points
- `crates/desktop/ui/src/features/logs/LogsView.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/logs/LogRows.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/logs/logs.css` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/processes/ProcessesView.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/jobs/JobsView.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/jobs/WorkflowEditor.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/services/log-mapping.ts` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/application/src/log_explorer.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `docs/evidence/desktop-ux/08-log-navigation.md` (proposed) — Publish the versioned execution and verification handoff.
- `docs/evidence/desktop-ux/07-workflow-edit.md` (proposed) — Consume the preceding contract and actual verification command manifest.

## Execution Plan
### Stage 1 — Establish the bounded baseline
- Starts when: `docs/evidence/desktop-ux/07-workflow-edit.md` is available with prerequisite readiness and its recorded native commands, contracts and remaining limitations; inspect the current checkout before reproducing this child.
- Work: Inspect all process/job/step log entry routes and capture query state before/after context navigation and automatic updates. Read applicable package AGENTS.md files before the first edit.
- No-op when: All Desired Outcome and Acceptance Criteria already hold on the current revision with the required native and final-consumer evidence.
- No-op handoff: Publish current-revision proof and `prerequisite_ready: true` at `docs/evidence/desktop-ux/08-log-navigation.md` (proposed) for docs/briefs/2026-10-27-fix-desktop-ux-09-presentation.md; the parent may skip edits but must retain required integration gates. A failed proof activates Stage 2 instead of waiving the concern.
- Deliverable: A baseline section at `docs/evidence/desktop-ux/08-log-navigation.md` (proposed) with scenario IDs, expected/actual behavior, reproduction mode and any disproved candidate.
- Verify: `Inspect the predecessor handoff and execute its bounded baseline scenarios`; Inputs: this child's named entry points and `docs/evidence/desktop-ux/07-workflow-edit.md`; Expected: Every child scenario has a reproducible baseline or a specific disproof with evidence.
- Ends when:
  - [x] Every in-scope concern is assigned a concrete scenario and evidence mode.
  - [x] The first edit and preserved contracts are identified, or the complete no-change proof is recorded.
- Handoff: Stage 2 receives the baseline, fixture inputs and contract boundaries at `docs/evidence/desktop-ux/08-log-navigation.md` (proposed).
- Replan when: The desired query state cannot be preserved without changing serialized log identity, grouping or snapshot semantics. Retain the existing contract and return the specific additive need to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

### Stage 2 — Complete the cohesive change
- Starts when: Stage 1 has recorded the baseline at `docs/evidence/desktop-ux/08-log-navigation.md` and no required contract decision is unresolved.
- Work: Unify log query/navigation state and raw inspection while preserving backend snapshot, grouping and export contracts. From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.
- Deliverable: The scoped implementation plus its case-specific assertions and exact invocation records, linked from `docs/evidence/desktop-ux/08-log-navigation.md` (proposed).
- Verify: `Inspect changed consumers and run the recorded relevant checks`; Inputs: the scoped diff and this child's scenario IDs; Expected: typecheck and applicable checks exit 0 and the changed value/behavior reaches the final consumer.
- Ends when:
  - [x] All In Scope behavior is implemented or specifically proved already satisfied.
  - [x] Shared callers consume the same contract and failures preserve user intent and valid prior state.
  - [x] Actual commands, exit codes and environment warnings are recorded.
- Handoff: Stage 3 receives the integrated diff, runnable scenarios and result locations from `docs/evidence/desktop-ux/08-log-navigation.md` (proposed).
- Replan when: The desired query state cannot be preserved without changing serialized log identity, grouping or snapshot semantics. Retain the existing contract and return the specific additive need to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.
- Worker decision: Choose local component/hook names and minimal compatible test dependencies within these contracts; do not widen product behavior or introduce another test framework without a concrete need.

### Stage 3 — Prove the native outcome and hand off
- Starts when: Stage 2 checks and all affected consumer updates are available on the same revision.
- Work: Use the exact cwd/command/scenario selector published in `docs/evidence/desktop-ux/01-test-harness.md`; do not invent a command that has not been implemented. Collect native evidence from the real app and label any mocked lane explicitly. Evaluate every Side Effect Checkpoint and Acceptance Criterion against this saved revision.
- Deliverable: Final `docs/evidence/desktop-ux/08-log-navigation.md` (proposed) containing revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits, plus an explicit prerequisite readiness decision.
- Verify: `Inspect the complete scenario report and replay the exact recorded command for the affected native lane`; Inputs: this child's Acceptance Criteria and its native fixture set; Expected: every required row has executed evidence and no missing, mocked-only or skipped native row is reported as a pass.
- Ends when:
  - [x] Each scenario result is attributed to the actual revision, fixture and evidence mode.
  - [x] Isolation/cleanup and cross-consumer side effects are verified, with unavailable observations explicit.
  - [x] The next consumer can locate the exact commands and contract changes without reconstructing this conversation.
- Handoff: docs/briefs/2026-10-27-fix-desktop-ux-09-presentation.md receives `docs/evidence/desktop-ux/08-log-navigation.md` (proposed), including the runnable command manifest, verified contracts and remaining limitations.
- Replan when: The desired query state cannot be preserved without changing serialized log identity, grouping or snapshot semantics. Retain the existing contract and return the specific additive need to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

## Side Effect Checkpoints
- [x] Existing HTTP and invoke mapping consumers still observe the same public keys, IDs and error contracts.
- [x] User inputs and inactive drafts are not silently normalized, lost or applied to a different object.
- [x] Existing cancellation, concurrency and revision protections are preserved through the final consumer.
- [x] Native and mocked fixtures remain separated, real user state is untouched and owned test processes/ports are cleaned up.
- [x] From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.

## Acceptance Criteria
- [x] LOG-01: Process, job and selected-step entry links produce the stated source/run/step and filter scope, including INFO records when the UI promises all levels.
- [x] LOG-02: Multi-day records visibly identify the date in the configured display timezone without requiring a pointer tooltip.
- [x] LOG-03: Reading an expanded group or older page remains stable when new records arrive; refresh controls accurately describe active/paused behavior.
- [x] LOG-04: Inspector/context/Back restores the preceding query and position, and source changes never retain another source's run choices or rows.
- [x] LOG-05: Custom-range and snapshot-paged exports contain exactly the selected boundaries, with partial-source failures reported rather than hidden.
- [x] LOG-06: Raw copy and inspection agree across process preview and explorer; the 10,000-entry fixture remains bounded and navigable.
- [x] LOG-07: A failed query is not presented solely as no matching logs; reset/retry actions preserve the meaningful input.
- [x] `docs/evidence/desktop-ux/08-log-navigation.md` records revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits; no remaining required scenario is hidden as an optional follow-up.

## Open Questions
- None — the user selected comprehensive desktop UX improvement and direct Tauri verification; compatibility is preserved, optional expansions are explicitly deferred, and technical choices are bounded above.
