# [test] Establish isolated native Tauri UX tests

## Work Type
test

## Current State (As-Is)
- [confirmed] The inspected baseline is `748537710413f224be9f25a65d114426b33c0770` on `feat/desktop-parity`; no source changes were made during the review.
- [confirmed] `crates/desktop/ui/package.json` has typecheck/build and `test:follow-events`, but no WDIO runner; existing parity tests stub `window.__TAURI_INTERNALS__.invoke` and do not launch a native WebView.
- [confirmed] `crates/desktop/src/main.rs::build_host` permits the embedded runtime only with debug assertions and `MSV_DESKTOP_EMBEDDED_DEV=1`; the release branch deliberately returns an error.
- [confirmed] `crates/daemon/src/owner.rs::debug_or_canonical_root` accepts `MSV_DAEMON_TEST_DATA_DIR` in debug builds. Desktop setup independently calls `data_dir()` for the devBridge discovery file, so setting the owner root alone does not prove complete isolation.
- [confirmed] `crates/desktop/tauri.conf.json` defines a 1100x720 window and production CSP; `Cargo.lock` resolves Tauri 2.12.5, while the UI manifest declares API ^2.12.4.
- [confirmed] Official Tauri WebDriver guidance inspected on 2026-10-27 recommends `@wdio/tauri-service`; its embedded provider supports macOS. Direct official `tauri-driver` is not the macOS route.

## Desired Outcome (To-Be)
- Provide a documented, repeatable native macOS test host using the existing React application and actual invoke handlers.
- Separate renderer/mocked-command assertions from real IPC/storage/process observations and native OS interaction evidence.

## Scope
### In Scope
- Adopt the documented embedded WDIO provider with pinned compatible test dependencies; inspect the installed service's actual plugin requirements before wiring optional bridge features.
- Gate both Rust plugins, frontend test imports, capability additions, fixture controls and launch settings behind an explicit test configuration. Ordinary debug and release builds must not acquire a WebDriver server or additional execute permissions.
- Launch only with a fresh task-owned data/config/log/discovery root and unique loopback ports. Fail before launch if any resolved path points at canonical user state. Route or suppress the desktop devBridge discovery write in test mode.
- Build the real debug app with bundled frontend assets and real helper binaries using the repository build route. Preserve the existing release-host rejection rather than bypassing it to get tests running.
- Provide typed fixtures for stopped/running processes, jobs, workflows, logs, delayed replies, failures and partial outcomes. Keep real-IPC fixtures separate from mocks, and restore original IPC implementations before native proof.
- Create a scenario inventory mapping CMD, INPUT, SAVE, SCHEDULE, STATE, FLOW, LOG and VISUAL cases to children 02-09. Record pre-fix native failures without silently skipping them or treating the baseline as a product pass.
- Record exact build/run commands, cwd, dependency versions, app PID, native WebView/build identity, fixture roots, command mocks, screenshots/logs and cleanup results in the handoff.
- Probe driver capabilities for focus, keyboard, window resize, file dialogs and IME. Schedule actual OS-level observations where WebDriver cannot exercise the native interaction; synthetic composition events alone are not IME proof.
- Retain and run the existing HTTP/invoke parity and event deduplication checks. Report socket/startup warnings as environment evidence instead of hiding them behind exit code 0.
- Prove IPC interception reaches the module-imported invoke function used by createInvokeClient, not only a test's direct global API call. Restore the same path before checking real backend state.
### Out of Scope
- [hard] Changing the production service-owner/release policy, canonical user state or existing public wire identity contracts as a shortcut for this child.
- [deferred] Signed/release installed-owner enablement, service upgrades and cross-platform release certification.

## Constraints
- This brief implements the approved desktop UX review and task-related Tauri test scope. Keep user-visible prose in Korean and preserve exact identifiers, logs and user-supplied values.
- Keep `/api/v1`, snake_case DTO keys, camelCase UI mappings, existing invoke command/argument names, definition/instance/run IDs, workflow node/edge/route IDs and expected-config/revision checks compatible.
- Use only task-owned fixtures and temporary data/config/log roots. Never read production credentials or mutate existing user processes/jobs/services for a test.
- Native WebView with mocked IPC, actual invoke/backend evidence and native OS observation are distinct results. A browser/mock pass must not satisfy a required native observation.
- Keep test plugins, frontend imports, execute permissions, fixtures and listeners out of normal builds. Preserve production CSP and the current release-host gate.
- Add only task-relevant tests and necessary development dependencies authorized by the user's Tauri-testing request. Do not introduce unrelated lint/formatter configuration.
- Do not commit or push without a separate user request. If a worktree is needed, use only `/Volumes/Samsung_990_PRO/worktrees/my_supervisor/<name>` while the volume is mounted and do not remove it without asking.
- Use the embedded provider as the selected macOS route; do not introduce the paid CrabNebula route, external credentials or Windows/Linux product work.
- Do not copy guide scaffolding over the repository. The user explicitly authorized task-related Tauri tests; unrelated lint/formatter/framework setup remains excluded.
- Do not assume `appEnv` or other undocumented service options. Use the installed version's supported launch interface and record it.
- Use `restoreAllMocks` semantics when returning to real IPC; clearing call history alone is insufficient.
- Prefer optional Cargo test dependencies and an explicit non-default feature plus a separate test config/capability overlay; record all differences from the ordinary app configuration.

## Related Files / Entry Points
- `crates/desktop/Cargo.toml` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/src/main.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/tauri.conf.json` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/capabilities/main.json` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/build.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/package.json` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/tests/operations-parity.test.mjs` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/tests/follow-events.test.mjs` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/daemon/src/owner.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/daemon/src/lib.rs` — Read the named behavior in Current State and keep changes within this child's scope.
- `scripts/build-local.sh` — Read the named behavior in Current State and keep changes within this child's scope.
- `crates/desktop/ui/tests/e2e` (proposed) — Add only the bounded artifact required by this child.
- `crates/desktop/ui/wdio.conf.ts` (proposed) — Add only the bounded artifact required by this child.
- `crates/desktop/tauri.e2e.conf.json` (proposed) — Add only the bounded artifact required by this child.
- `docs/evidence/desktop-ux/01-test-harness.md` (proposed) — Publish the versioned execution and verification handoff.
- [Tauri test overview](https://v2.tauri.app/develop/tests/) — Consult the official source inspected on 2026-10-27 and verify compatibility with the installed versions.
- [Tauri WebDriver route](https://v2.tauri.app/develop/tests/webdriver/) — Consult the official source inspected on 2026-10-27 and verify compatibility with the installed versions.
- [WDIO plugin setup](https://webdriver.io/docs/desktop-testing/tauri/plugin-setup) — Consult the official source inspected on 2026-10-27 and verify compatibility with the installed versions.
- [WDIO configuration](https://webdriver.io/docs/desktop-testing/tauri/configuration) — Consult the official source inspected on 2026-10-27 and verify compatibility with the installed versions.
- [WDIO mock restoration](https://webdriver.io/docs/desktop-testing/tauri/api) — Consult the official source inspected on 2026-10-27 and verify compatibility with the installed versions.

## Execution Plan
### Stage 1 — Establish the bounded baseline
- Starts when: The user has authorized this briefset and task-related Tauri tests; inspect baseline 748537710413f224be9f25a65d114426b33c0770 and resolve an isolated native test root before launching anything.
- Work: Inspect the current scripts and reproduce one command-order case, one delimiter case and the CSS column computation in a native test session after the isolated host is ready. Read applicable package AGENTS.md files before the first edit.
- No-op when: All Desired Outcome and Acceptance Criteria already hold on the current revision with the required native and final-consumer evidence.
- No-op handoff: Publish current-revision proof and `prerequisite_ready: true` at `docs/evidence/desktop-ux/01-test-harness.md` (proposed) for docs/briefs/2026-10-27-fix-desktop-ux-02-command-edit.md; the parent may skip edits but must retain required integration gates. A failed proof activates Stage 2 instead of waiving the concern.
- Deliverable: A baseline section at `docs/evidence/desktop-ux/01-test-harness.md` (proposed) with scenario IDs, expected/actual behavior, reproduction mode and any disproved candidate.
- Verify: `Inspect the current manifests, host gates and source-linked scenario inventory`; Inputs: this child's named entry points and the existing package manifests and build_host/data-root branches; Expected: Supported native route, isolated paths and a complete scenario-to-owner inventory are recorded without a product-pass claim.
- Ends when:
  - [x] Every in-scope concern is assigned a concrete scenario and evidence mode.
  - [x] The first edit and preserved contracts are identified, or the complete no-change proof is recorded.
- Handoff: Stage 2 receives the baseline, fixture inputs and contract boundaries at `docs/evidence/desktop-ux/01-test-harness.md` (proposed).
- Replan when: The embedded provider cannot start with the pinned dependencies, isolation cannot be proved, or testing requires weakening production security. Stop dependent native execution, return the concrete incompatibility to the parent and resolve a bounded harness correction without claiming browser equivalence. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

### Stage 2 — Complete the cohesive change
- Starts when: Stage 1 has recorded the baseline at `docs/evidence/desktop-ux/01-test-harness.md` and no required contract decision is unresolved.
- Work: Implement the bounded test host, fixtures, native smoke scenario and command/result manifest. Do not fix product UX defects in this child. From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.
- Deliverable: The scoped implementation plus its case-specific assertions and exact invocation records, linked from `docs/evidence/desktop-ux/01-test-harness.md` (proposed).
- Verify: `Inspect changed consumers and run the recorded relevant checks`; Inputs: the scoped diff and this child's scenario IDs; Expected: typecheck and applicable checks exit 0 and the changed value/behavior reaches the final consumer.
- Ends when:
  - [x] All In Scope behavior is implemented or specifically proved already satisfied.
  - [x] Shared callers consume the same contract and failures preserve user intent and valid prior state.
  - [x] Actual commands, exit codes and environment warnings are recorded.
- Handoff: Stage 3 receives the integrated diff, runnable scenarios and result locations from `docs/evidence/desktop-ux/01-test-harness.md` (proposed).
- Replan when: The embedded provider cannot start with the pinned dependencies, isolation cannot be proved, or testing requires weakening production security. Stop dependent native execution, return the concrete incompatibility to the parent and resolve a bounded harness correction without claiming browser equivalence. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.
- Worker decision: Choose local component/hook names and minimal compatible test dependencies within these contracts; do not widen product behavior or introduce another test framework without a concrete need.

### Stage 3 — Prove the native outcome and hand off
- Starts when: Stage 2 checks and all affected consumer updates are available on the same revision.
- Work: Use the exact cwd/command/scenario selector published in `docs/evidence/desktop-ux/01-test-harness.md`; do not invent a command that has not been implemented. Collect native evidence from the real app and label any mocked lane explicitly. Evaluate every Side Effect Checkpoint and Acceptance Criterion against this saved revision.
- Deliverable: Final `docs/evidence/desktop-ux/01-test-harness.md` (proposed) containing revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits, plus an explicit prerequisite readiness decision.
- Verify: `Inspect the complete scenario report and replay the exact recorded command for the affected native lane`; Inputs: this child's Acceptance Criteria and its native fixture set; Expected: every required row has executed evidence and no missing, mocked-only or skipped native row is reported as a pass.
- Ends when:
  - [x] Each scenario result is attributed to the actual revision, fixture and evidence mode.
  - [x] Isolation/cleanup and cross-consumer side effects are verified, with unavailable observations explicit.
  - [x] The next consumer can locate the exact commands and contract changes without reconstructing this conversation.
- Handoff: docs/briefs/2026-10-27-fix-desktop-ux-02-command-edit.md receives `docs/evidence/desktop-ux/01-test-harness.md` (proposed), including the runnable command manifest, verified contracts and remaining limitations.
- Replan when: The embedded provider cannot start with the pinned dependencies, isolation cannot be proved, or testing requires weakening production security. Stop dependent native execution, return the concrete incompatibility to the parent and resolve a bounded harness correction without claiming browser equivalence. If a required proof fails, stop this child's acceptance and dependent work, return to the parent, assign bounded correction/re-verification and revise handoffs before resuming.

## Side Effect Checkpoints
- [x] Existing HTTP and invoke mapping consumers still observe the same public keys, IDs and error contracts.
- [x] User inputs and inactive drafts are not silently normalized, lost or applied to a different object.
- [x] Existing cancellation, concurrency and revision protections are preserved through the final consumer.
- [x] Native and mocked fixtures remain separated, real user state is untouched and owned test processes/ports are cleaned up.
- [x] From repository root run `pnpm --dir crates/desktop/ui typecheck` and the relevant existing checks recorded in the harness manifest. Run `cargo check --workspace` after any Rust change, and `pnpm --dir crates/desktop/ui build` when validating compiled UI/CSS.

## Acceptance Criteria
- [x] HARNESS-01: A non-empty native smoke run launches `my-supervisor`, identifies `main`, and obtains real daemon status through invoke from the isolated runtime.
- [x] HARNESS-02: A controlled mock rejection reaches the UI, then full mock restoration is proved by reading a real fixture value through the same adapter.
- [x] HARNESS-03: Startup failure, failed scenario, normal completion and interrupted run all stop owned helpers and close test ports without touching other processes or user state.
- [x] HARNESS-04: The baseline manifest names each known failure, exact fixture and owning child; expected pre-fix failures do not masquerade as passing regressions.
- [x] HARNESS-05: A non-test build/config inspection shows no test frontend import, WDIO capability, fixture endpoint or listener; record any release-host execution limitation separately.
- [x] `docs/evidence/desktop-ux/01-test-harness.md` records revision, exact command/cwd, tool and native build identity, scenario ID, fixture and mocks, before/after result, final-consumer assertion, evidence links, cleanup and remaining limits; no remaining required scenario is hidden as an optional follow-up.

## Open Questions
- None — the user selected comprehensive desktop UX improvement and direct Tauri verification; compatibility is preserved, optional expansions are explicitly deferred, and technical choices are bounded above.
