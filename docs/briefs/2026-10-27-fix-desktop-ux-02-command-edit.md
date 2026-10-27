# [fix] Preserve commands during safe editing

## Work Type
fix

## Current State (As-Is)
- [confirmed] Snapshot: commit `748537710413f224be9f25a65d114426b33c0770` on `feat/desktop-parity`, inspected on 2026-10-27 — Evidence: `git rev-parse HEAD`, `git branch --show-current` and the stable source locators below.
- [confirmed] `command-model.ts::rowsToArguments` sorts rows into runtime/entry/script groups. Browser review reproduced `pnpm run start -- --port 3000` becoming `pnpm -- --port 4000 run start` after changing only the port.
- [confirmed] `CommandEditor` strips the first parsed word when it equals the executable name, and keeps parse failures inside its own error state while parent Apply/Save controls remain enabled.
- [confirmed] `CommandEditor` automatically requests executable discovery and help on mount/change. `MacLifecycle::executables` executes version probes; `command_help` executes the program or script with help arguments.
- [confirmed] `ProcessEditor`, `JobEditor`, command workflow steps and exec health checks all consume this shared editor.
- [inferred] A script that ignores `--help` can execute startup side effects while its settings are opened. Confirm with a harmless sentinel executable that writes only inside the fixture root.

## Reproduction
- At the baseline, enter `run start -- --port 3000` for pnpm, blur, then change only the option value to 4000; observe regrouped argv.
- Enter an unmatched quote, blur and observe that parent Apply remains enabled while prior valid argv remains in state.
- Open a fixture script editor with no user probe action and observe whether the sentinel file is created.
- Use baseline 748537710413f224be9f25a65d114426b33c0770 and the predecessor's isolated fixtures. Browser observations were repeatable where identified; confirm native frequency and backend-only cases in Stage 1 before changing their implementation.

## Desired Outcome (To-Be)
- Keep the exact intended program and ordered argv through editing, saving and final process execution.
- Make command inspection non-executing by default and make invalid/uncommitted text block consumers from saving stale arguments.

## Scope
### In Scope
- Preserve original argv order, `--` boundaries, repeated flags, empty arguments, Unicode, commas, quotes and executable-name-looking positional arguments.
- Define separate program and argv editing semantics; expose an accurate final command preview without silently stripping or regrouping tokens.
- Propagate draft validity and pending raw edits to every parent save/execute consumer, including workflow steps and health checks.
- Remove automatic arbitrary program/script execution from editor mount, typing, program selection and help discovery. Keep runtime installation an explicit separate action.
- Offer an explicit help/version probe action when needed, with clear execution semantics, loading/error/cancel behavior and bounded cleanup; do not assume help/version flags are side-effect free.
- Keep completion candidates keyboard reachable and preserve the caret when help responses arrive; stale responses must not replace the current command draft.
- If moving the editor out of the processes feature improves shared ownership, move all imports atomically and preserve the public prop contract or update every consumer together.
### Out of Scope
- [hard] Changing the production service-owner/release policy, canonical user state or existing public wire identity contracts as a shortcut for this child.
- [deferred] A new shell parser language, automatic script rewriting and a general runtime package manager redesign.

## Constraints
- This brief implements the approved desktop UX review and task-related Tauri test scope. Keep user-visible prose in Korean and preserve exact identifiers, logs and user-supplied values.
- Keep `/api/v1`, snake_case DTO keys, camelCase UI mappings, existing invoke command/argument names, definition/instance/run IDs, workflow node/edge/route IDs and expected-config/revision checks compatible.
- Use only task-owned fixtures and temporary data/config/log roots. Never read production credentials or mutate existing user processes/jobs/services for a test.
- Native WebView with mocked IPC, actual invoke/backend evidence and native OS observation are distinct results. A browser/mock pass must not satisfy a required native observation.
- Keep test plugins, frontend imports, execute permissions, fixtures and listeners out of normal builds. Preserve production CSP and the current release-host gate.
- Add only task-relevant tests and necessary development dependencies authorized by the user's Tauri-testing request. Do not introduce unrelated lint/formatter configuration.
- Do not commit or push without a separate user request. If a worktree is needed, use only `/Volumes/Samsung_990_PRO/worktrees/my_supervisor/<name>` while the volume is mounted and do not remove it without asking.

## Related Files / Entry Points
- `crates/desktop/ui/src/features/processes/CommandEditor.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/processes/command-model.ts` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/processes/ProcessEditor.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/jobs/JobEditor.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/jobs/StepSettings.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/platform/macos/src/developer_tools.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/application/src/developer_tools.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `docs/evidence/desktop-ux/02-command-edit.md` (proposed) — Publish the versioned execution and verification handoff.
- `docs/evidence/desktop-ux/01-test-harness.md` (proposed) — Consume the preceding contract and actual verification command manifest.

## Execution Plan
### Stage 1 — Establish the bounded baseline
- Starts when: `docs/evidence/desktop-ux/01-test-harness.md` is available with prerequisite readiness and its recorded native commands, contracts and remaining limitations; inspect the current checkout before reproducing this child.
- Work: Capture the existing argv mutation, stale-valid-value save path and unsolicited probe count before changing the editor. Read applicable package AGENTS.md files before the first edit.
- No-op when: All Desired Outcome and Acceptance Criteria already hold on the current revision with the required native and final-consumer evidence.
- No-op handoff: Publish current-revision proof and `prerequisite_ready: true` at `docs/evidence/desktop-ux/02-command-edit.md` (proposed) for docs/briefs/2026-10-27-feat-desktop-ux-03-input-flow.md; the parent may skip edits but must retain required integration gates. A failed proof activates Stage 2 instead of waiving the concern.
- Deliverable: A baseline section at `docs/evidence/desktop-ux/02-command-edit.md` (proposed) with scenario IDs, expected/actual behavior, reproduction mode and any disproved candidate.
- Verify: `Inspect the predecessor handoff and execute its bounded baseline scenarios`; Inputs: this child's named entry points and `docs/evidence/desktop-ux/01-test-harness.md`; Expected: Every child scenario has a reproducible baseline or a specific disproof with evidence.
- Ends when:
  - [x] Every in-scope concern is assigned a concrete scenario and evidence mode.
  - [x] The first edit and preserved contracts are identified, or the complete no-change proof is recorded.
- Handoff: Stage 2 receives the baseline, fixture inputs and contract boundaries at `docs/evidence/desktop-ux/02-command-edit.md` (proposed).
- Replan when: Exact argv cannot be preserved without changing persisted config or the execution contract. Stop the affected consumer path and return the compatibility issue to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

### Stage 2 — Complete the cohesive change
- Starts when: Stage 1 has recorded the baseline at `docs/evidence/desktop-ux/02-command-edit.md` and no required contract decision is unresolved.
- Work: Repair the shared command model and validity/probe contract together, then wire every parent consumer to that contract. From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.
- Deliverable: The scoped implementation plus its case-specific assertions and exact invocation records, linked from `docs/evidence/desktop-ux/02-command-edit.md` (proposed).
- Verify: `Inspect changed consumers and run the recorded relevant checks`; Inputs: the scoped diff and this child's scenario IDs; Expected: typecheck and applicable checks exit 0 and the changed value/behavior reaches the final consumer.
- Ends when:
  - [x] All In Scope behavior is implemented or specifically proved already satisfied.
  - [x] Shared callers consume the same contract and failures preserve user intent and valid prior state.
  - [x] Actual commands, exit codes and environment warnings are recorded.
- Handoff: Stage 3 receives the integrated diff, runnable scenarios and result locations from `docs/evidence/desktop-ux/02-command-edit.md` (proposed).
- Replan when: Exact argv cannot be preserved without changing persisted config or the execution contract. Stop the affected consumer path and return the compatibility issue to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.
- Worker decision: Choose local component/hook names and minimal compatible test dependencies within these contracts; do not widen product behavior or introduce another test framework without a concrete need.

### Stage 3 — Prove the native outcome and hand off
- Starts when: Stage 2 checks and all affected consumer updates are available on the same revision.
- Work: Use the exact cwd/command/scenario selector published in `docs/evidence/desktop-ux/01-test-harness.md`; do not invent a command that has not been implemented. Collect native evidence from the real app and label any mocked lane explicitly. Evaluate every Side Effect Checkpoint and Acceptance Criterion against this saved revision.
- Deliverable: Final `docs/evidence/desktop-ux/02-command-edit.md` (proposed) containing revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits, plus an explicit prerequisite readiness decision.
- Verify: `Inspect the complete scenario report and replay the exact recorded command for the affected native lane`; Inputs: this child's Acceptance Criteria and its native fixture set; Expected: every required row has executed evidence and no missing, mocked-only or skipped native row is reported as a pass.
- Ends when:
  - [x] Each scenario result is attributed to the actual revision, fixture and evidence mode.
  - [x] Isolation/cleanup and cross-consumer side effects are verified, with unavailable observations explicit.
  - [x] The next consumer can locate the exact commands and contract changes without reconstructing this conversation.
- Handoff: docs/briefs/2026-10-27-feat-desktop-ux-03-input-flow.md receives `docs/evidence/desktop-ux/02-command-edit.md` (proposed), including the runnable command manifest, verified contracts and remaining limitations.
- Replan when: Exact argv cannot be preserved without changing persisted config or the execution contract. Stop the affected consumer path and return the compatibility issue to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

## Side Effect Checkpoints
- [x] Existing HTTP and invoke mapping consumers still observe the same public keys, IDs and error contracts.
- [x] User inputs and inactive drafts are not silently normalized, lost or applied to a different object.
- [x] Existing cancellation, concurrency and revision protections are preserved through the final consumer.
- [x] Native and mocked fixtures remain separated, real user state is untouched and owned test processes/ports are cleaned up.
- [x] From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.

## Acceptance Criteria
- [x] CMD-01: Editing only 3000 to 4000 yields argv `["run","start","--","--port","4000"]` in the UI, saved config and fixture process argv output.
- [x] CMD-02: Empty arguments, embedded commas, Unicode, quotes, repeated flags, and a first positional argument equal to the executable name round-trip unchanged.
- [x] CMD-03: Malformed quotes/escapes keep the visible draft and explanation, send no save/run request, and become saveable only after correction.
- [x] CMD-04: Mounting, changing tabs and typing produce zero sentinel executions; an explicit probe is the only action that may execute it.
- [x] CMD-05: Slow help results and keyboard completion do not rewrite a newer draft; failed/cancelled probes leave no owned child running.
- [x] CMD-06: Repeat representative valid and invalid cases in process, job, workflow-command and exec-health consumers.
- [x] `docs/evidence/desktop-ux/02-command-edit.md` records revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits; no remaining required scenario is hidden as an optional follow-up.

## Open Questions
- None — the user selected comprehensive desktop UX improvement and direct Tauri verification; compatibility is preserved, optional expansions are explicitly deferred, and technical choices are bounded above.
