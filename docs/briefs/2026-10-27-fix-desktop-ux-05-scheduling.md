# [fix] Align schedule editing with execution defaults

## Work Type
fix

## Current State (As-Is)
- [confirmed] Snapshot: commit `748537710413f224be9f25a65d114426b33c0770` on `feat/desktop-parity`, inspected on 2026-10-27 — Evidence: `git rev-parse HEAD`, `git branch --show-current` and the stable source locators below.
- [confirmed] `JobEditor` fills a missing timezone with UTC, while `job_config_to_job` and the existing schedule contract resolve an omitted timezone to the host's local IANA zone.
- [confirmed] `JobEditor` displays 3600 for an omitted timeout without inserting it into the saved value; `workflow_engine::execute` falls back to 86400 when the job timeout is absent.
- [confirmed] `preview` is retained after schedule changes, and the user must click the preview action again to replace old dates.
- [confirmed] `shared/time.ts` already resolves wall-clock input and rejects nonexistent DST times; keep that conversion contract.

## Reproduction
- Create a new cron job on an Asia/Seoul host without selecting a timezone; inspect the UI-generated UTC value versus the documented local default.
- Save a new job without touching the displayed 3600-second timeout and inspect the omitted wire value.
- Preview a schedule, edit its expression and observe that the old occurrence list remains.
- Use baseline 748537710413f224be9f25a65d114426b33c0770 and the predecessor's isolated fixtures. Browser observations were repeatable where identified; confirm native frequency and backend-only cases in Stage 1 before changing their implementation.

## Desired Outcome (To-Be)
- Display and save the same schedule timezone and timeout semantics across GUI, HTTP and invoke.
- Make the interpreted schedule and next occurrences understandable and current.

## Scope
### In Scope
- Preserve existing explicit timezone values and honor server authority for omitted new-job defaults; do not silently force UTC or derive schedule semantics from display timezone.
- Expose the scheduling timezone separately from display timezone and show a concise human-readable interpretation alongside cron.
- Remove invented timeout display defaults. Represent omitted/default, explicit value and any supported no-limit mode truthfully and preserve them through serialization.
- Invalidate prior previews on schedule/timezone changes and show whether results are loading, current or failed. Discard stale replies by request identity.
- Provide common schedule presets as a reversible editing aid while retaining the exact cron editor and unsupported-expression visibility.
- Preserve one-shot timezone conversion, future-time validation and DST diagnostics; do not silently resolve invalid wall-clock input.
- Retain trigger-type drafts supplied by child 03 and editor transaction behavior supplied by child 04.
### Out of Scope
- [hard] Changing the production service-owner/release policy, canonical user state or existing public wire identity contracts as a shortcut for this child.
- [deferred] New scheduler policies, altered DST resolution policy and unrelated retry/misfire engine redesign.

## Constraints
- This brief implements the approved desktop UX review and task-related Tauri test scope. Keep user-visible prose in Korean and preserve exact identifiers, logs and user-supplied values.
- Keep `/api/v1`, snake_case DTO keys, camelCase UI mappings, existing invoke command/argument names, definition/instance/run IDs, workflow node/edge/route IDs and expected-config/revision checks compatible.
- Use only task-owned fixtures and temporary data/config/log roots. Never read production credentials or mutate existing user processes/jobs/services for a test.
- Native WebView with mocked IPC, actual invoke/backend evidence and native OS observation are distinct results. A browser/mock pass must not satisfy a required native observation.
- Keep test plugins, frontend imports, execute permissions, fixtures and listeners out of normal builds. Preserve production CSP and the current release-host gate.
- Add only task-relevant tests and necessary development dependencies authorized by the user's Tauri-testing request. Do not introduce unrelated lint/formatter configuration.
- Do not commit or push without a separate user request. If a worktree is needed, use only `/Volumes/Samsung_990_PRO/worktrees/my_supervisor/<name>` while the volume is mounted and do not remove it without asking.

## Related Files / Entry Points
- `crates/desktop/ui/src/features/jobs/JobEditor.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/jobs/JobsView.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/services/wire-mapping.ts` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/shared/time.ts` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/shared/display-time.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/infra/http/src/mapping.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/application/src/workflow_engine.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `docs/evidence/macos-prod-04-schedule-contract.md` — Read the named behavior in Current State and keep changes within this child's scope.
- `docs/evidence/desktop-ux/05-scheduling.md` (proposed) — Publish the versioned execution and verification handoff.
- `docs/evidence/desktop-ux/04-save-lifecycle.md` (proposed) — Consume the preceding contract and actual verification command manifest.

## Execution Plan
### Stage 1 — Establish the bounded baseline
- Starts when: `docs/evidence/desktop-ux/04-save-lifecycle.md` is available with prerequisite readiness and its recorded native commands, contracts and remaining limitations; inspect the current checkout before reproducing this child.
- Work: Pin omitted/default and explicit values through form state, wire mapping, saved job and actual scheduler/runner consumers. Read applicable package AGENTS.md files before the first edit.
- No-op when: All Desired Outcome and Acceptance Criteria already hold on the current revision with the required native and final-consumer evidence.
- No-op handoff: Publish current-revision proof and `prerequisite_ready: true` at `docs/evidence/desktop-ux/05-scheduling.md` (proposed) for docs/briefs/2026-10-27-fix-desktop-ux-06-operator-state.md; the parent may skip edits but must retain required integration gates. A failed proof activates Stage 2 instead of waiving the concern.
- Deliverable: A baseline section at `docs/evidence/desktop-ux/05-scheduling.md` (proposed) with scenario IDs, expected/actual behavior, reproduction mode and any disproved candidate.
- Verify: `Inspect the predecessor handoff and execute its bounded baseline scenarios`; Inputs: this child's named entry points and `docs/evidence/desktop-ux/04-save-lifecycle.md`; Expected: Every child scenario has a reproducible baseline or a specific disproof with evidence.
- Ends when:
  - [x] Every in-scope concern is assigned a concrete scenario and evidence mode.
  - [x] The first edit and preserved contracts are identified, or the complete no-change proof is recorded.
- Handoff: Stage 2 receives the baseline, fixture inputs and contract boundaries at `docs/evidence/desktop-ux/05-scheduling.md` (proposed).
- Replan when: The backend cannot expose enough effective-default information without an additive API change. Document the smallest additive contract and update all consumers together before continuing. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

### Stage 2 — Complete the cohesive change
- Starts when: Stage 1 has recorded the baseline at `docs/evidence/desktop-ux/05-scheduling.md` and no required contract decision is unresolved.
- Work: Align defaults, timezone selection and preview freshness; use existing preview/conversion APIs instead of duplicating scheduling calculations. From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.
- Deliverable: The scoped implementation plus its case-specific assertions and exact invocation records, linked from `docs/evidence/desktop-ux/05-scheduling.md` (proposed).
- Verify: `Inspect changed consumers and run the recorded relevant checks`; Inputs: the scoped diff and this child's scenario IDs; Expected: typecheck and applicable checks exit 0 and the changed value/behavior reaches the final consumer.
- Ends when:
  - [x] All In Scope behavior is implemented or specifically proved already satisfied.
  - [x] Shared callers consume the same contract and failures preserve user intent and valid prior state.
  - [x] Actual commands, exit codes and environment warnings are recorded.
- Handoff: Stage 3 receives the integrated diff, runnable scenarios and result locations from `docs/evidence/desktop-ux/05-scheduling.md` (proposed).
- Replan when: The backend cannot expose enough effective-default information without an additive API change. Document the smallest additive contract and update all consumers together before continuing. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.
- Worker decision: Choose local component/hook names and minimal compatible test dependencies within these contracts; do not widen product behavior or introduce another test framework without a concrete need.

### Stage 3 — Prove the native outcome and hand off
- Starts when: Stage 2 checks and all affected consumer updates are available on the same revision.
- Work: Use the exact cwd/command/scenario selector published in `docs/evidence/desktop-ux/01-test-harness.md`; do not invent a command that has not been implemented. Collect native evidence from the real app and label any mocked lane explicitly. Evaluate every Side Effect Checkpoint and Acceptance Criterion against this saved revision.
- Deliverable: Final `docs/evidence/desktop-ux/05-scheduling.md` (proposed) containing revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits, plus an explicit prerequisite readiness decision.
- Verify: `Inspect the complete scenario report and replay the exact recorded command for the affected native lane`; Inputs: this child's Acceptance Criteria and its native fixture set; Expected: every required row has executed evidence and no missing, mocked-only or skipped native row is reported as a pass.
- Ends when:
  - [x] Each scenario result is attributed to the actual revision, fixture and evidence mode.
  - [x] Isolation/cleanup and cross-consumer side effects are verified, with unavailable observations explicit.
  - [x] The next consumer can locate the exact commands and contract changes without reconstructing this conversation.
- Handoff: docs/briefs/2026-10-27-fix-desktop-ux-06-operator-state.md receives `docs/evidence/desktop-ux/05-scheduling.md` (proposed), including the runnable command manifest, verified contracts and remaining limitations.
- Replan when: The backend cannot expose enough effective-default information without an additive API change. Document the smallest additive contract and update all consumers together before continuing. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

## Side Effect Checkpoints
- [x] Existing HTTP and invoke mapping consumers still observe the same public keys, IDs and error contracts.
- [x] User inputs and inactive drafts are not silently normalized, lost or applied to a different object.
- [x] Existing cancellation, concurrency and revision protections are preserved through the final consumer.
- [x] Native and mocked fixtures remain separated, real user state is untouched and owned test processes/ports are cleaned up.
- [x] From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.

## Acceptance Criteria
- [x] SCHEDULE-01: On an Asia/Seoul fixture host, an omitted timezone resolves consistently through GUI, HTTP and invoke; an explicit UTC or America/Los_Angeles timezone remains unchanged.
- [x] SCHEDULE-02: The displayed timeout matches persisted and runner-effective semantics before and after saving without editing the field.
- [x] SCHEDULE-03: Preview A resolving after preview B cannot replace B; changing the expression invalidates A immediately.
- [x] SCHEDULE-04: A one-shot time survives display-zone changes as the same instant; invalid/nonexistent DST input remains visible with a useful error.
- [x] SCHEDULE-05: Preset-to-cron-to-preset navigation preserves meaning, and the saved schedule agrees with the displayed next occurrences.
- [x] SCHEDULE-06: Existing timezone/misfire/trigger identity fields and HTTP/invoke serialization remain compatible.
- [x] `docs/evidence/desktop-ux/05-scheduling.md` records revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits; no remaining required scenario is hidden as an optional follow-up.

## Open Questions
- None — the user selected comprehensive desktop UX improvement and direct Tauri verification; compatibility is preserved, optional expansions are explicitly deferred, and technical choices are bounded above.
