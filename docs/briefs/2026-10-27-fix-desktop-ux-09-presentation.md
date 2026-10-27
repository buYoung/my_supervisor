# [fix] Unify layout ownership and accessible controls

## Work Type
fix

## Current State (As-Is)
- [confirmed] Snapshot: commit `748537710413f224be9f25a65d114426b33c0770` on `feat/desktop-parity`, inspected on 2026-10-27 — Evidence: `git rev-parse HEAD`, `git branch --show-current` and the stable source locators below.
- [confirmed] `process-controls.css` declares a 182px action column, but later workspace.css declarations override it to 72px. Review observed the override in computed browser styles and the production CSS bundle.
- [confirmed] The configured native window is 1100x720; existing <=1250px layouts place detail below the entire list.
- [confirmed] `theme.css` dark primary text over panel computes to about 3.46:1; buttons and links use that semantic token for small text.
- [confirmed] `Panel`, `PanelHeader`, `Field`, `DataTable` and `TableCell` have no current src consumers, while feature views use separate markup/CSS.
- [confirmed] Reduced-motion rules and labelled shared buttons already exist in index.css/primitives.tsx; preserve them.

## Reproduction
- At 1100x720 load process rows with start/stop/restart controls and inspect the final grid: the 182px intent is replaced by 72px.
- Use keyboard-only navigation through a job row action and search field; inspect action isolation and focus indication.
- Inspect dark small primary text on the panel and compare the computed contrast.
- Use baseline 748537710413f224be9f25a65d114426b33c0770 and the predecessor's isolated fixtures. Browser observations were repeatable where identified; confirm native frequency and backend-only cases in Stage 1 before changing their implementation.

## Desired Outcome (To-Be)
- Give each visual rule one owner and keep primary controls usable in the default native window.
- Apply consistent focus, semantics, feedback and readable content to the integrated flows.

## Scope
### In Scope
- Remove competing process-grid/modal/control declarations and establish one scoped owner for each rule; verify the built CSS and native computed layout.
- Keep lifecycle actions and selected details visible/reachable at 1100x720 and resized desktop windows without accidental page-wide horizontal overflow.
- Fix nested row/button keyboard behavior, labelled tabs/segmented controls, visible search focus, field descriptions/errors and context-appropriate dialog initial/return focus.
- Separate semantic text and filled-action colors as needed to meet readable contrast in light/dark themes; preserve reduced motion and non-color state indicators.
- Unify inline loading/error/notice patterns and descriptive empty-state actions using the actual feature states supplied by preceding children.
- Remove redundant page titles/status repetition and non-actionable development copy while retaining a single discoverable daemon state and convenient theme access.
- Use consistent Korean terminology for system theme, process management, schedules, local draft changes and actual execution effects; keep identifiers/raw error details available when useful.
- Review unused primitives and duplicated raw panels for a bounded reuse/removal decision, preserving real consumers and avoiding a new generic table framework.
- Keep mockups outside the runtime component inventory and do not treat prototype duplication as production duplication.
### Out of Scope
- [hard] Changing the production service-owner/release policy, canonical user state or existing public wire identity contracts as a shortcut for this child.
- [deferred] Brand redesign, mobile-first layouts and a new UI component dependency stack.

## Constraints
- This brief implements the approved desktop UX review and task-related Tauri test scope. Keep user-visible prose in Korean and preserve exact identifiers, logs and user-supplied values.
- Keep `/api/v1`, snake_case DTO keys, camelCase UI mappings, existing invoke command/argument names, definition/instance/run IDs, workflow node/edge/route IDs and expected-config/revision checks compatible.
- Use only task-owned fixtures and temporary data/config/log roots. Never read production credentials or mutate existing user processes/jobs/services for a test.
- Native WebView with mocked IPC, actual invoke/backend evidence and native OS observation are distinct results. A browser/mock pass must not satisfy a required native observation.
- Keep test plugins, frontend imports, execute permissions, fixtures and listeners out of normal builds. Preserve production CSP and the current release-host gate.
- Add only task-relevant tests and necessary development dependencies authorized by the user's Tauri-testing request. Do not introduce unrelated lint/formatter configuration.
- Do not commit or push without a separate user request. If a worktree is needed, use only `/Volumes/Samsung_990_PRO/worktrees/my_supervisor/<name>` while the volume is mounted and do not remove it without asking.

## Related Files / Entry Points
- `crates/desktop/ui/src/main.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/index.css` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/shared/workspace.css` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/shared/theme.css` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/processes/process-controls.css` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/components/ui/primitives.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/components/ui/Modal.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/App.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/logs/logs.css` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/src/features/settings/SettingsView.tsx` — Read the named behavior in Current State and keep changes within this child's scope.
- `docs/evidence/desktop-ux/09-presentation.md` (proposed) — Publish the versioned execution and verification handoff.
- `docs/evidence/desktop-ux/08-log-navigation.md` (proposed) — Consume the preceding contract and actual verification command manifest.
- [W3C contrast criterion](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html) — Consult the official source inspected on 2026-10-27 and verify compatibility with the installed versions.
- [W3C dialog pattern](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/) — Consult the official source inspected on 2026-10-27 and verify compatibility with the installed versions.

## Execution Plan
### Stage 1 — Establish the bounded baseline
- Starts when: `docs/evidence/desktop-ux/08-log-navigation.md` is available with prerequisite readiness and its recorded native commands, contracts and remaining limitations; inspect the current checkout before reproducing this child.
- Work: Record representative native screenshots and computed bounds/contrast on the built frontend before CSS ownership changes. Read applicable package AGENTS.md files before the first edit.
- No-op when: All Desired Outcome and Acceptance Criteria already hold on the current revision with the required native and final-consumer evidence.
- No-op handoff: Publish current-revision proof and `prerequisite_ready: true` at `docs/evidence/desktop-ux/09-presentation.md` (proposed) for docs/briefs/2026-10-27-test-desktop-ux-10-native-gates.md; the parent may skip edits but must retain required integration gates. A failed proof activates Stage 2 instead of waiving the concern.
- Deliverable: A baseline section at `docs/evidence/desktop-ux/09-presentation.md` (proposed) with scenario IDs, expected/actual behavior, reproduction mode and any disproved candidate.
- Verify: `Inspect the predecessor handoff and execute its bounded baseline scenarios`; Inputs: this child's named entry points and `docs/evidence/desktop-ux/08-log-navigation.md`; Expected: Every child scenario has a reproducible baseline or a specific disproof with evidence.
- Ends when:
  - [x] Every in-scope concern is assigned a concrete scenario and evidence mode.
  - [x] The first edit and preserved contracts are identified, or the complete no-change proof is recorded.
- Handoff: Stage 2 receives the baseline, fixture inputs and contract boundaries at `docs/evidence/desktop-ux/09-presentation.md` (proposed).
- Replan when: A visual cleanup changes an execution contract or requires a wholesale design-system/library migration. Keep the functional UI intact and return the expanded change to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

### Stage 2 — Complete the cohesive change
- Starts when: Stage 1 has recorded the baseline at `docs/evidence/desktop-ux/09-presentation.md` and no required contract decision is unresolved.
- Work: Consolidate the shared visual and accessibility behavior without changing operations, DTOs or feature policy. From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.
- Deliverable: The scoped implementation plus its case-specific assertions and exact invocation records, linked from `docs/evidence/desktop-ux/09-presentation.md` (proposed).
- Verify: `Inspect changed consumers and run the recorded relevant checks`; Inputs: the scoped diff and this child's scenario IDs; Expected: typecheck and applicable checks exit 0 and the changed value/behavior reaches the final consumer.
- Ends when:
  - [x] All In Scope behavior is implemented or specifically proved already satisfied.
  - [x] Shared callers consume the same contract and failures preserve user intent and valid prior state.
  - [x] Actual commands, exit codes and environment warnings are recorded.
- Handoff: Stage 3 receives the integrated diff, runnable scenarios and result locations from `docs/evidence/desktop-ux/09-presentation.md` (proposed).
- Replan when: A visual cleanup changes an execution contract or requires a wholesale design-system/library migration. Keep the functional UI intact and return the expanded change to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.
- Worker decision: Choose local component/hook names and minimal compatible test dependencies within these contracts; do not widen product behavior or introduce another test framework without a concrete need.

### Stage 3 — Prove the native outcome and hand off
- Starts when: Stage 2 checks and all affected consumer updates are available on the same revision.
- Work: Use the exact cwd/command/scenario selector published in `docs/evidence/desktop-ux/01-test-harness.md`; do not invent a command that has not been implemented. Collect native evidence from the real app and label any mocked lane explicitly. Evaluate every Side Effect Checkpoint and Acceptance Criterion against this saved revision.
- Deliverable: Final `docs/evidence/desktop-ux/09-presentation.md` (proposed) containing revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits, plus an explicit prerequisite readiness decision.
- Verify: `Inspect the complete scenario report and replay the exact recorded command for the affected native lane`; Inputs: this child's Acceptance Criteria and its native fixture set; Expected: every required row has executed evidence and no missing, mocked-only or skipped native row is reported as a pass.
- Ends when:
  - [x] Each scenario result is attributed to the actual revision, fixture and evidence mode.
  - [x] Isolation/cleanup and cross-consumer side effects are verified, with unavailable observations explicit.
  - [x] The next consumer can locate the exact commands and contract changes without reconstructing this conversation.
- Handoff: docs/briefs/2026-10-27-test-desktop-ux-10-native-gates.md receives `docs/evidence/desktop-ux/09-presentation.md` (proposed), including the runnable command manifest, verified contracts and remaining limitations.
- Replan when: A visual cleanup changes an execution contract or requires a wholesale design-system/library migration. Keep the functional UI intact and return the expanded change to the parent. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

## Side Effect Checkpoints
- [x] Existing HTTP and invoke mapping consumers still observe the same public keys, IDs and error contracts.
- [x] User inputs and inactive drafts are not silently normalized, lost or applied to a different object.
- [x] Existing cancellation, concurrency and revision protections are preserved through the final consumer.
- [x] Native and mocked fixtures remain separated, real user state is untouched and owned test processes/ports are cleaned up.
- [x] From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.

## Acceptance Criteria
- [x] VISUAL-01: Built assets and native computed styles retain sufficient process action space with no overlap for running/stopped/transitioning rows and long names.
- [x] VISUAL-02: At 1100x720 and narrower/wider resizes, opening a selected detail visibly reveals it and returns focus predictably on close.
- [x] VISUAL-03: Keyboard-only navigation can reach and activate controls with visible focus; an inner action does not trigger row navigation.
- [x] VISUAL-04: Normal enabled text meets 4.5:1 contrast in both themes, with applicable large-text exceptions identified; reduced-motion settings remain respected.
- [x] VISUAL-05: Error, empty, loading, stale and partial states use consistent readable content and accessible announcements without stealing focus on each poll.
- [x] VISUAL-06: Theme shortcuts stay consistent with Settings, raw technical details remain available, and duplicate unused primitives/styles have an explicit retained-or-removed rationale.
- [x] `docs/evidence/desktop-ux/09-presentation.md` records revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits; no remaining required scenario is hidden as an optional follow-up.

## Open Questions
- None — the user selected comprehensive desktop UX improvement and direct Tauri verification; compatibility is preserved, optional expansions are explicitly deferred, and technical choices are bounded above.
