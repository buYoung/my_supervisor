# [feat] Build durable schedule dispatch

## Work Type
feat

## Current State (As-Is)
- [confirmed] As of `a1c845c` on `main`, `TokioScheduler` holds trigger definitions and timer handles in memory and sends fire events through an unbounded single-consumer channel — Evidence: `TokioScheduler` fields and `register()` in `crates/infra/scheduler/src/lib.rs`.
- [confirmed] Bootstrap re-registers each timer from the current time and marks persisted `Pending` or `Running` runs cancelled; it does not enumerate schedule occurrences missed while the daemon was unavailable — Evidence: `OperationsFacade::bootstrap()` in `crates/application/src/facade.rs`.
- [confirmed] The scheduler event includes `scheduled_at`, but `run_scheduler_loop()` passes only `job_name` into `on_schedule_tick()`, and `dispatch_run()` writes the current clock value — Evidence: scheduler select branch, `on_schedule_tick()`, and `dispatch_run()` in `crates/application/src/facade.rs`.
- [confirmed] Queue overlap is held only in `ActiveRunRegistry.queued_by_job`, so queued work does not survive daemon restart — Evidence: `ActiveRunState`, `enqueue()`, `finish()`, and bootstrap cancellation behavior in `crates/application/src/facade.rs`.
- [confirmed] Dependency terminal events have a durable transient outbox and lag reconciliation, while timed occurrences have no equivalent durable claim or delivery record — Evidence: `transient_terminal_outbox`, `deliver_transient_terminal_events()`, and `reconcile_dependency_completions()`.
- [inferred] An in-memory timer/channel can remain a wake-up optimization, but it cannot be the authority for production delivery because a crash between fire and persistence loses the occurrence — Confirm by: terminating the daemon after timer fire and before `save_run`, then reopening the same database.

## Desired Outcome (To-Be)
- A durable schedule cursor and occurrence ledger, not an in-memory timer event, determine what work is due.
- Bootstrap applies each job's `skip`, `run_once`, or bounded `catch_up` policy and never creates duplicate occurrences after repeated restarts.
- Original scheduled time flows unchanged into every occurrence and attempt.
- Retry attempts use durable backoff, resume after restart, and publish one final dependency outcome.
- Queued runs survive daemon restart and obey per-job concurrency, queue capacity, overflow behavior, and global admission limits.
- Shutdown, deletion, config replacement, and cancellation drain or retain durable work according to explicit ownership rules.

## Scope
### In Scope
- Implement atomic due-occurrence claim and schedule-cursor advancement.
- Reconcile missed cron, interval, and one-shot occurrences at bootstrap and during runtime.
- Persist queued admission, retry timing, attempt relationship, and final occurrence outcome.
- Enforce per-job and global concurrency/queue limits with observable rejection reasons.
- Preserve dependency deduplication and deliver downstream work only after final occurrence completion.
- Make in-memory timers replaceable wake-up hints rather than delivery authority.
### Out of Scope
- [deferred] Metrics retention and alert routing belong to `docs/briefs/2026-07-25-feat-macos-prod-07-telemetry-alerts.md`.
- [deferred] Operator policy forms belong to `docs/briefs/2026-07-25-feat-macos-prod-08-operator-ui.md`.
- [hard] Do not execute unlimited catch-up or unlimited queued/parallel work.
- [hard] Do not add new test files or test cases without separate user approval.

## Constraints
- Start from the schedule contract at `crates/core/src/domain/job.rs`.
- Claim occurrence identity and advance durable cursor in one transaction or an idempotent equivalent.
- Default missed-run handling is one recovered occurrence; bounded catch-up requires an explicit maximum.
- A retry never creates a second logical occurrence and never triggers dependency consumers before final outcome.
- Preserve cancellation signals and active-run ownership through existing cleanup and terminal-outbox boundaries.

## Related Files / Entry Points
- `crates/application/src/facade.rs` — start at bootstrap, scheduler loop, dispatch, active-run registry, and dependency completion.
- `crates/core/src/ports/scheduler.rs` — separate wake-up calculation from durable delivery authority.
- `crates/core/src/ports/repository.rs` — add atomic occurrence claim, queue, retry, and cursor operations.
- `crates/infra/scheduler/src/lib.rs` — emit wake-ups and deterministic occurrence candidates.
- `crates/infra/sqlite/src/lib.rs` — persist cursor, occurrence, attempt, and admission state.
- `crates/application/src/runner.rs` — preserve terminal attempt and cancellation ownership.
- `crates/application/src/events.rs` — expose misfire, retry, overflow, and final occurrence events.
- `docs/evidence/macos-prod-05-durable-dispatch.md` (proposed) — record claim/dispatch crash boundaries, misfire, queue, retry, and dependency evidence.

## Execution Plan
### Stage 1 — Make due occurrence claiming authoritative
- Starts when: The deterministic schedule contract is available at `crates/core/src/domain/job.rs`.
- Work: Introduce one atomic path that claims a due logical occurrence and advances or records its schedule cursor before dispatch.
- No-op when: Timed dispatch is already reconstructed entirely from durable cursor and occurrence state, and duplicate claims are rejected.
- No-op handoff: Parent receives confirmed durable authority at `crates/core/src/ports/repository.rs`; continue only if an in-memory event can be lost without losing the occurrence.
- Deliverable: Atomic occurrence, cursor, queue, retry, and final-outcome authority exposed through `crates/core/src/ports/repository.rs`; `crates/core/src/ports/scheduler.rs` remains a wake-up calculation boundary.
- Verify: `bounded occurrence crash-boundary inspection`; Inputs: scheduler port, repository transaction, and bootstrap path; Expected: every crash point yields either no claim or one recoverable claimed occurrence, never an unrecorded due event.
- Ends when:
  - [ ] Timers only wake the reconciler and cannot be the sole record of due work.
  - [ ] Repeated reconciliation returns the same claimed occurrence rather than a duplicate.
- Handoff: Stage 2 receives atomic occurrence and cursor operations.
- Replan when: Claim and cursor state cannot share an atomic SQLite boundary; stop and introduce a durable outbox/saga before dispatch work.

### Stage 2 — Reconcile missed work and preserve schedule time
- Starts when: Stage 1 can claim occurrences idempotently.
- Work: Apply timezone/DST/misfire policy at startup and runtime while preserving original scheduled timestamps.
- Deliverable: Deterministic `skip`, `run_once`, and bounded `catch_up` reconciliation.
- Verify: `bounded fixed-time reconciliation inspection`; Inputs: one cron, interval, and one-shot schedule across zero, one, and many missed occurrences; Expected: the exact configured occurrence count, unique keys, and original UTC schedule times.
- Ends when:
  - [ ] Repeated daemon restart cannot duplicate recovered work.
  - [ ] Catch-up obeys per-job maximum and global admission limits.
  - [ ] One-shot completion cannot be rearmed after its occurrence is claimed.
- Handoff: Stage 3 receives durable due occurrences and exact scheduled times.
- Replan when: Interval semantics cannot distinguish fixed-rate from fixed-delay expectations; preserve current fixed-delay compatibility and add an explicit new mode instead of silently changing it.

### Stage 3 — Persist retries, queues, and final outcomes
- Starts when: Durable occurrence creation and missed-run reconciliation are stable.
- Work: Persist admission state and retry timing, restore queued work after restart, and emit one final occurrence outcome to dependency processing.
- Deliverable: Restart-safe retry, queue, concurrency, and dependency dispatch behavior.
- Verify: `cargo test --workspace`; Inputs: existing run, cancellation, deletion, recovery, dependency, SQLite, and scheduler suites plus bounded restart at each claim/dispatch/queue/retry/final-outcome boundary; Expected: exit code `0`, queued/retry state survives with the same occurrence, and one final dependency claim is created; record at `docs/evidence/macos-prod-05-durable-dispatch.md`.
- Ends when:
  - [ ] Per-job and global limits are enforced before child spawn.
  - [ ] Overflow, cancellation, timeout, failed attempt, exhausted retry, and final success are distinguishable.
  - [ ] Deletion and config replacement cannot attach old queued work to a reused job name.
- Handoff: Parent and log-retention work receive durable occurrence/run identity and dispatch authority at `crates/core/src/ports/repository.rs`.
- Replan when: Existing active-run deletion or cleanup ownership conflicts with durable queue restoration; stop and extend the existing deletion/cleanup saga rather than bypassing it.

## Side Effect Checkpoints
- [ ] Existing manual triggers still create one immediate logical occurrence and return an addressable run.
- [ ] Existing overlap behavior maps to the new admission policy without silently increasing concurrency.
- [ ] Dependency signatures remain idempotent across event lag, restart, retry, and reused job names.
- [ ] Job deletion journals drain or remove all queued and retry attempts belonging to the original `JobId`.
- [ ] Terminal cleanup and external event outbox retain exactly-once acknowledgement identity.
- [ ] Scheduler registration rollback still restores the previous durable definition and wake-up state.

## Acceptance Criteria
- [ ] Killing and restarting the daemon at each claim/dispatch boundary produces no lost or duplicate logical occurrence.
- [ ] `run_once` creates at most one recovered occurrence after any downtime length; bounded `catch_up` never exceeds its configured maximum.
- [ ] A queued or backoff retry attempt resumes after restart with the same occurrence relationship and original scheduled time.
- [ ] Per-job and global concurrency/queue limits prevent unbounded child creation and return an explicit overflow result.
- [ ] Dependency jobs run once only after the upstream occurrence reaches its final policy outcome.
- [ ] Existing `cargo test --workspace` passes outside the socket-restricted sandbox.

## Open Questions
- None — production dispatch defaults are durable, bounded, and idempotent.
