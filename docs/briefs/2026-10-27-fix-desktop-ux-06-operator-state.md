# [fix] Make operator state and action results reliable

## Work Type
fix

## Current State (As-Is)
- [confirmed] Snapshot: commit `748537710413f224be9f25a65d114426b33c0770` on `feat/desktop-parity`, inspected on 2026-10-27 — Evidence: `git rev-parse HEAD`, `git branch --show-current` and the stable source locators below.
- [confirmed] `App` and feature views own independent polls; the header refresh only refreshes the App resources and does not refresh the active feature's list/history.
- [confirmed] `usePolledResource` retains last successful data but has no request generation ordering; views frequently convert null into an empty array and ignore isLoading.
- [confirmed] `ProcessesView::act` ignores returned operation data; rolling restart can return `Ok(operation)` with phase failed_closed and failed instance outcomes.
- [confirmed] `DaemonView` drops payload.from/to and colors events by substrings in the event type; process.state_changed does not identify a crash in its name.
- [confirmed] Selected process logs and Job runs are not consistently cleared/keyed when the selected object changes, and instance refresh depends on aggregate PID/state.
- [confirmed] `JobsView` loads full workflow definitions for group metadata and recalculates connected groups for individual rows.

## Reproduction
- Disconnect the fixture backend and observe an error alongside zero counts and an empty-success message.
- Refresh Jobs from the header and inspect that its own resource is not invalidated.
- Return a failed_closed rolling outcome and inspect the generic request notice; emit a process.state_changed event with to=crashed and inspect its presentation.
- Use baseline 748537710413f224be9f25a65d114426b33c0770 and the predecessor's isolated fixtures. Browser observations were repeatable where identified; confirm native frequency and backend-only cases in Stage 1 before changing their implementation.

## Desired Outcome (To-Be)
- Keep loading, empty, filtered-empty, disconnected, stale and partial states distinguishable.
- Show per-target operation results and preserve the operator's selection while data updates.

## Scope
### In Scope
- Share authoritative resource snapshots/invalidation between the header and feature consumers; make refresh affect the active view and expose the actual refresh result.
- Reject stale poll replies, avoid unnecessary concurrent refreshes and retain labelled last-good data during transient failure.
- Key details/logs/history/instances by resource identity; clear or mark pending data when changing target and refresh instance-level changes without relying only on aggregate PID/state.
- Consume rolling operation phase/completed/outcomes/failedStage/retryable and report completed, failed and not-attempted instances accurately; use available return data before inventing a new API.
- Track busy state per affected target/action so an unrelated process is not blocked. Respect the job's configured overlap policy rather than silently changing concurrency behavior.
- Render daemon state transitions and guard failures from payload fields, with Korean action/result wording and a useful route to logs or relevant settings.
- Provide search-empty feedback, filter reset, stable selected rows, and an explicit detail action accessible by mouse and keyboard; stop nested lifecycle keys from triggering row navigation.
- Preserve list/search/detail return context, including Back navigation, while keeping real IDs/names in existing route contracts.
- Use bounded list/page contracts where supported and compute group metadata once per relevant change. Record actual data-size behavior without inventing latency targets.
- Make destructive persisted removal state-specific, distinguish stopping from deleting, and preserve existing force/dependency protections.
- Show guard.lastError and meaningful restart causes near unhealthy states instead of requiring users to infer the failure from a generic badge.
### Out of Scope
- [hard] Changing the production service-owner/release policy, canonical user state or existing public wire identity contracts as a shortcut for this child.
- [deferred] A new observability dashboard, alert-rule management, daemon maintenance UI and cross-platform host lifecycle changes.

## Constraints
- This brief implements the approved desktop UX review and task-related Tauri test scope. Keep user-visible prose in Korean and preserve exact identifiers, logs and user-supplied values.
- Keep `/api/v1`, snake_case DTO keys, camelCase UI mappings, existing invoke command/argument names, definition/instance/run IDs, workflow node/edge/route IDs and expected-config/revision checks compatible.
- Use only task-owned fixtures and temporary data/config/log roots. Never read production credentials or mutate existing user processes/jobs/services for a test.
- Native WebView with mocked IPC, actual invoke/backend evidence and native OS observation are distinct results. A browser/mock pass must not satisfy a required native observation.
- Keep test plugins, frontend imports, execute permissions, fixtures and listeners out of normal builds. Preserve production CSP and the current release-host gate.
- Add only task-relevant tests and necessary development dependencies authorized by the user's Tauri-testing request. Do not introduce unrelated lint/formatter configuration.
- Do not commit or push without a separate user request. If a worktree is needed, use only `/Volumes/Samsung_990_PRO/worktrees/my_supervisor/<name>` while the volume is mounted and do not remove it without asking.

## Related Files / Entry Points
- `crates/desktop/ui/src/services/use-operations.ts` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/App.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/processes/ProcessesView.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/jobs/JobsView.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/daemon/DaemonView.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/settings/SettingsView.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/services/operations-client.ts` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/services/wire-mapping.ts` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/infra/http/src/ws.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/application/src/facade.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `docs/evidence/desktop-ux/06-operator-state.md` (proposed) — Publish the versioned execution and verification handoff.
- `docs/evidence/desktop-ux/05-scheduling.md` (proposed) — Consume the preceding contract and actual verification command manifest.

## Execution Plan
### Stage 1 — Establish the bounded baseline
- Starts when: `docs/evidence/desktop-ux/05-scheduling.md` is available with prerequisite readiness and its recorded native commands, contracts and remaining limitations; inspect the current checkout before reproducing this child.
- Work: Capture stale-response ordering, object switching, header refresh and partial operation scenarios before state ownership changes. Read applicable package AGENTS.md files before the first edit.
- No-op when: All Desired Outcome and Acceptance Criteria already hold on the current revision with the required native and final-consumer evidence.
- No-op handoff: Publish current-revision proof and `prerequisite_ready: true` at `docs/evidence/desktop-ux/06-operator-state.md` (proposed) for docs/briefs/2026-10-27-feat-desktop-ux-07-workflow-edit.md; the parent may skip edits but must retain required integration gates. A failed proof activates Stage 2 instead of waiving the concern.
- Deliverable: A baseline section at `docs/evidence/desktop-ux/06-operator-state.md` (proposed) with scenario IDs, expected/actual behavior, reproduction mode and any disproved candidate.
- Verify: `Inspect the predecessor handoff and execute its bounded baseline scenarios`; Inputs: this child's named entry points and `docs/evidence/desktop-ux/05-scheduling.md`; Expected: Every child scenario has a reproducible baseline or a specific disproof with evidence.
- Ends when:
  - [x] Every in-scope concern is assigned a concrete scenario and evidence mode.
  - [x] The first edit and preserved contracts are identified, or the complete no-change proof is recorded.
- Handoff: Stage 2 receives the baseline, fixture inputs and contract boundaries at `docs/evidence/desktop-ux/06-operator-state.md` (proposed).
- Replan when: Required state cannot be obtained from existing responses or bounded list APIs. Return an additive-contract proposal to the parent and preserve all existing consumers. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

### Stage 2 — Complete the cohesive change
- Starts when: Stage 1 has recorded the baseline at `docs/evidence/desktop-ux/06-operator-state.md` and no required contract decision is unresolved.
- Work: Repair shared resource/action state and update every list/detail/header consumer in the same integration step. From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.
- Deliverable: The scoped implementation plus its case-specific assertions and exact invocation records, linked from `docs/evidence/desktop-ux/06-operator-state.md` (proposed).
- Verify: `Inspect changed consumers and run the recorded relevant checks`; Inputs: the scoped diff and this child's scenario IDs; Expected: typecheck and applicable checks exit 0 and the changed value/behavior reaches the final consumer.
- Ends when:
  - [x] All In Scope behavior is implemented or specifically proved already satisfied.
  - [x] Shared callers consume the same contract and failures preserve user intent and valid prior state.
  - [x] Actual commands, exit codes and environment warnings are recorded.
- Handoff: Stage 3 receives the integrated diff, runnable scenarios and result locations from `docs/evidence/desktop-ux/06-operator-state.md` (proposed).
- Replan when: Required state cannot be obtained from existing responses or bounded list APIs. Return an additive-contract proposal to the parent and preserve all existing consumers. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.
- Worker decision: Choose local component/hook names and minimal compatible test dependencies within these contracts; do not widen product behavior or introduce another test framework without a concrete need.

### Stage 3 — Prove the native outcome and hand off
- Starts when: Stage 2 checks and all affected consumer updates are available on the same revision.
- Work: Use the exact cwd/command/scenario selector published in `docs/evidence/desktop-ux/01-test-harness.md`; do not invent a command that has not been implemented. Collect native evidence from the real app and label any mocked lane explicitly. Evaluate every Side Effect Checkpoint and Acceptance Criterion against this saved revision.
- Deliverable: Final `docs/evidence/desktop-ux/06-operator-state.md` (proposed) containing revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits, plus an explicit prerequisite readiness decision.
- Verify: `Inspect the complete scenario report and replay the exact recorded command for the affected native lane`; Inputs: this child's Acceptance Criteria and its native fixture set; Expected: every required row has executed evidence and no missing, mocked-only or skipped native row is reported as a pass.
- Ends when:
  - [x] Each scenario result is attributed to the actual revision, fixture and evidence mode.
  - [x] Isolation/cleanup and cross-consumer side effects are verified, with unavailable observations explicit.
  - [x] The next consumer can locate the exact commands and contract changes without reconstructing this conversation.
- Handoff: docs/briefs/2026-10-27-feat-desktop-ux-07-workflow-edit.md receives `docs/evidence/desktop-ux/06-operator-state.md` (proposed), including the runnable command manifest, verified contracts and remaining limitations.
- Replan when: Required state cannot be obtained from existing responses or bounded list APIs. Return an additive-contract proposal to the parent and preserve all existing consumers. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

## Side Effect Checkpoints
- [x] Existing HTTP and invoke mapping consumers still observe the same public keys, IDs and error contracts.
- [x] User inputs and inactive drafts are not silently normalized, lost or applied to a different object.
- [x] Existing cancellation, concurrency and revision protections are preserved through the final consumer.
- [x] Native and mocked fixtures remain separated, real user state is untouched and owned test processes/ports are cleaned up.
- [x] From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.

## Acceptance Criteria
- [x] STATE-01: Initial load, no registrations, zero search results, offline, stale data and partial data have distinct content and valid next actions.
- [x] STATE-02: Header refresh updates the active list/detail; a delayed old response cannot overwrite a newer fetch or mutation result.
- [x] STATE-03: Switching A to B never shows A's logs/runs/instances as B, including B read failure and repeated polling.
- [x] STATE-04: A real isolated rolling readiness failure displays failed/not-attempted instance outcomes and the retained healthy instance state.
- [x] STATE-05: Crash/stop/start events display the correct transition and severity; unknown/unsupported state remains explicit.
- [x] STATE-06: Keyboard lifecycle actions execute only their target action; selecting a row preserves focus and exposes its detail at the current window size.
- [x] STATE-07: A slow action on A leaves supported actions on B usable, and retry does not repeat a completed side effect.
- [x] STATE-08: The existing 250-job/50-instance data-shape fixtures remain searchable and inspectable with explicit pagination/partial boundaries; record timings without making an unapproved performance pass threshold.
- [x] `docs/evidence/desktop-ux/06-operator-state.md` records revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits; no remaining required scenario is hidden as an optional follow-up.

## Open Questions
- None — the user selected comprehensive desktop UX improvement and direct Tauri verification; compatibility is preserved, optional expansions are explicitly deferred, and technical choices are bounded above.
