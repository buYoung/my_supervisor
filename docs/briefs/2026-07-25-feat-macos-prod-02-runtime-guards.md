# [feat] Enforce production runtime guardrails

## Work Type
feat

## Current State (As-Is)
- [confirmed] As of `a1c845c` on `main`, the Direct supervisor probes OS liveness, resets restart counters after a stable interval, and restarts crashed children with capped exponential backoff — Evidence: `run_process_supervisor_loop()`, `reconcile_direct_processes()`, and `restart_delay()` in `crates/application/src/facade.rs`.
- [confirmed] Resource sampling returns instantaneous CPU percentage and memory bytes but no policy consumes those values — Evidence: `LifecycleController::resource_usage`, `MacLifecycle::resource_usage`, and `OperationsFacade::build_status`.
- [confirmed] Direct children own a dedicated process group and graceful shutdown escalates to group kill, protecting descendant cleanup — Evidence: `ChildHandle.pgid`, macOS `spawn_child`, and `UnixShutdown` paths under `crates/platform/macos/src`.
- [confirmed] No process file-watch, liveness command, readiness check, memory ceiling, restart-cause history, or debounce policy appears in Rust or desktop source — Evidence: scoped search over `crates/**/*.{rs,ts,tsx,toml}` for those contract terms.
- [inferred] Reusing the current supervisor interval for every probe without per-check deadlines could let one slow health check delay crash and memory enforcement for unrelated processes — Confirm by: tracing one reconciliation cycle with a deliberately hanging health command.

## Desired Outcome (To-Be)
- Direct processes restart when configured files change or memory remains above a configured ceiling, using debounce and sustained-breach windows to avoid restart storms.
- Configured liveness checks can mark and restart an unhealthy process; readiness checks gate operator-visible readiness and later rolling-restart handoffs.
- Every policy-driven restart records a stable cause, timestamps, attempt state, and last check result.
- Policy evaluation is bounded so a slow or failing check cannot block supervision of other processes.
- Manual stop, delete, config apply, and daemon shutdown always cancel pending policy actions and never trigger an unintended restart.

## Scope
### In Scope
- Implement Direct-mode file-watch, memory-limit, liveness, and readiness evaluators.
- Integrate evaluators with the existing restart token, process lock, process-group shutdown, and backoff paths.
- Expose current readiness, health, memory breach, watch state, and last restart cause through application status/events.
- Define explicit unsupported or delegated behavior for SystemRegistered processes.
- Add bounded macOS manual verification scenarios using temporary commands and files.
### Out of Scope
- [deferred] Desired instance-count reconciliation and rolling group reload belong to `docs/briefs/2026-07-25-feat-macos-prod-03-multi-instance.md`.
- [deferred] Long-term metrics retention and alert delivery belong to `docs/briefs/2026-07-25-feat-macos-prod-07-telemetry-alerts.md`.
- [hard] Do not signal a PID or process group unless native generation and ownership checks still match.
- [hard] Do not add new test files or test cases without separate user approval.

## Constraints
- Start from the contract in `crates/core/src/domain/process.rs`.
- Preserve caller cancellation, manual stop intent, config-apply locks, and existing restart tokens.
- Watch is disabled, resource ceilings are absent, and health checks are absent for migrated definitions.
- Every external check has a timeout, output-size bound, and no shell interpolation unless the user explicitly configured a shell command.
- Add a filesystem-watching dependency only if native macOS event delivery cannot be implemented safely with existing dependencies; document why it is necessary.
- Worker decision: automatic triggers for one generation coalesce into one pending action; priority is memory breach, liveness failure, then file watch, while readiness only gates rollout unless restart-on-readiness-failure is explicit. Record every contributing cause.
- Worker decision: each external check owns a separate process group, captures bounded output, and on timeout/cancellation performs TERM, bounded drain, KILL, and reap before releasing its slot.
- Worker decision: file watch uses explicit roots, configurable recursion/exclusions, treats atomic replace/rename as change, excludes supervisor data/log paths, and after overflow/restart performs one bounded rescan before rearming; one debounce window yields one action.
- Worker decision: SystemRegistered watch/memory/liveness/readiness policies are reported `unsupported` and rejected before persistence; only launchd-owned restart/status remains delegated.
- Worker decision: restored check snapshots are historical/stale evidence only; readiness starts false/unknown for each daemon session and process generation until a fresh check succeeds, and multi-instance rollout cannot consume stale readiness.
- Worker decision: a resource-sampling error marks memory state unknown, resets the consecutive-breach window, emits bounded diagnostic evidence, and never requests restart; only consecutive successful over-limit samples count.

## Related Files / Entry Points
- `crates/application/src/facade.rs` — start at the supervisor reconciliation and pending-restart ownership paths.
- `crates/core/src/ports/lifecycle.rs` — expose only platform capabilities required for safe policy evaluation.
- `crates/platform/macos/src/lifecycle.rs` — preserve generation-safe resource sampling and child ownership.
- `crates/platform/macos/src/shutdown.rs` — preserve TERM-to-KILL process-group cleanup.
- `crates/application/src/events.rs` — add additive health and policy-action observations.
- `crates/core/src/ports/repository.rs` — persist the latest guardrail cause/check snapshot without moving restart authority.
- `crates/infra/sqlite/src/lib.rs` — restore latest guardrail evidence across daemon restart with additive migration.
- `crates/daemon/src/main.rs` — confirm the supervisor loop remains spawned exactly once.
- `docs/evidence/macos-prod-02-runtime-guards.md` (proposed) — record ownership and bounded macOS guardrail scenarios.

## Execution Plan
### Stage 1 — Pin policy ownership and cancellation
- Starts when: The production process contract is available at `crates/core/src/domain/process.rs`.
- Work: Map each policy trigger onto the existing process lock, restart token, shutdown intent, and config-apply boundaries before enabling new evaluators.
- No-op when: `reconcile_direct_processes()` already evaluates all configured guardrails with bounded checks and records stable causes.
- No-op handoff: Parent receives confirmed behavior at `crates/application/src/facade.rs`; continue to multi-instance work only if the inspection proves cancellation and policy ownership.
- Deliverable: A policy-state and cancellation design integrated into `crates/application/src/facade.rs`.
- Verify: `bounded ownership inspection`; Inputs: `reconcile_direct_processes()`, `stop_process()`, `remove_process()`, `apply_config()`, shutdown paths, and `crates/application/src/facade.rs`; Expected: one owner for each pending action and an explicit cancellation route for every manual operation.
- Ends when:
  - [ ] Policy actions cannot outlive a replaced process generation.
  - [ ] Manual and configuration operations take precedence over automatic actions.
- Handoff: Stage 2 receives the ownership map and policy state transitions.
- Replan when: Existing restart tokens cannot distinguish policy cause or instance identity; return to the process-contract brief before implementing evaluators.

### Stage 2 — Implement bounded evaluators
- Starts when: Stage 1 fixes policy ownership and cancellation.
- No-op when: The parent debug harness proves bounded watch/memory/liveness/readiness execution, coalescing, process-group check cleanup, and restart rehydration; record proof and continue to Stage 3 without evaluator edits.
- Work: Evaluate file changes, sustained memory breaches, liveness, and readiness independently with per-check timeouts, debounce, and bounded scheduling.
- Deliverable: Production guardrail evaluators connected to live Direct processes.
- Verify: `cargo check -p my-supervisor-application -p my-supervisor-platform-macos`; Inputs: application policy orchestration and macOS lifecycle capabilities; Expected: exit code `0` and no blocking check runs under a global synchronous lock.
- Ends when:
  - [ ] A configured policy produces one debounced action per qualifying condition.
  - [ ] A hanging or failing check times out without blocking other processes.
  - [ ] SystemRegistered policies are explicitly delegated or reported unsupported.
- Handoff: Stage 3 receives live evaluator outcomes and cause metadata.
- Replan when: Reliable file events require an external dependency or health execution requires a new port; add the narrow capability at the core boundary and recompile all implementers before continuing.

### Stage 3 — Integrate restart, status, and operator evidence
- Starts when: Evaluators emit bounded outcomes with stable process identity.
- No-op when: Status/cause persistence and the full cancellation/restart scenario matrix already pass; record proof and hand the guardrails to multi-instance work without integration edits.
- Work: Route unhealthy conditions through existing backoff and safe shutdown, persist the latest bounded readiness/check/breach/watch/restart-cause snapshot, rehydrate it on daemon restart, and expose it additively.
- Deliverable: Runtime guardrail behavior and operator evidence in `crates/application/src/facade.rs`.
- Automated gate: Run `cargo test --workspace`; Expected: exit code `0` before and after the manual inspection.
- Verify: `bounded macOS runtime-guard inspection`; Inputs: parent debug-harness commands plus watched root, atomic replace, overflow/rescan, aggregate-memory breach, timed-out descendant check, concurrent triggers, manual stop, force delete, config rollback, stale-generation replacement, daemon TERM/restart, and shutdown during each pending action; Expected: one coalesced action per window, reaped descendants, no post-cancel/stale action, restart restores the latest evidence but recreates only configured evaluators with no inherited sustained window; record commands/identities/counts/cleanup at `docs/evidence/macos-prod-02-runtime-guards.md`.
- Ends when:
  - [ ] Policy restarts use graceful group shutdown before force kill.
  - [ ] Status distinguishes running-but-not-ready, unhealthy, crashed, and stopped states without removing old state fields.
  - [ ] Restart causes remain visible after the immediate event.
- Handoff: `docs/briefs/2026-07-25-feat-macos-prod-03-multi-instance.md` and the parent receive the implemented guardrails at `crates/application/src/facade.rs`.
- Replan when: A real macOS scenario can restart a manually stopped process or signal a replacement generation; stop successors and correct identity/cancellation behavior first.

## Side Effect Checkpoints
- [ ] `stop_process()` during backoff still cancels automatic restart.
- [ ] Config apply and mode conversion do not leave watchers, health tasks, or memory actions attached to old definitions.
- [ ] Tied-child shutdown and detached-child recovery retain their current process-group cleanup guarantees.
- [ ] SystemRegistered restart remains owned by launchd and is not duplicated by the Direct supervisor.
- [ ] Resource sampling failure does not become a false memory breach.

## Acceptance Criteria
- [ ] A watched-path change causes exactly one restart after the configured debounce window.
- [ ] Memory must remain over the configured ceiling for the configured sustained window before one restart is requested.
- [ ] Liveness failure triggers the configured restart path; readiness failure blocks readiness without falsely reporting a crash.
- [ ] Manual stop, force delete, config replacement, and daemon shutdown cancel all pending policy actions.
- [ ] Existing `cargo test --workspace` passes outside the socket-restricted sandbox, and the manual macOS scenarios record expected process generations and restart causes.

## Open Questions
- None — production guardrail behavior is bounded by the process contract and existing lifecycle guarantees.
