# [feat] Add bounded telemetry and local operations alerts

## Work Type
feat

## Current State (As-Is)
- [confirmed] As of `a1c845c` on `main`, process status exposes current CPU and memory but no bounded metric history — Evidence: `ProcessStatus` and `OperationsFacade::build_status()`.
- [confirmed] Domain and WebSocket events exist, and terminal transient events have a durable outbox, but no queryable operator-event retention, alert rule, cooldown, acknowledgement, or recovery lifecycle exists — Evidence: `crates/application/src/events.rs`, `crates/infra/http/src/ws.rs`, and SQLite `transient_terminal_outbox`.
- [confirmed] Process restart, job run, timeout, cancellation, and dependency outcomes already pass through the application facade and event publisher — Evidence: `crates/application/src/facade.rs` and `crates/application/src/events.rs`.
- [confirmed] Daemon composition already selects macOS adapters and desktop can host the shared runtime, but there is no alert-delivery port or Notification Center adapter — Evidence: `crates/daemon/src/lib.rs` and scoped port search in `crates/core/src/ports`.
- [inferred] Delivering notifications directly inside supervision or schedule transitions would couple external delivery failure to authoritative state and recovery — Confirm by: trace every proposed delivery call and require the process/job transition to commit independently first.

## Desired Outcome (To-Be)
- Bounded process/instance metrics and operational events expose recent resource use, restart causes, health/readiness changes, rollout outcomes, schedule misfires, retries, overflow, and final failures.
- Alert rules support typed conditions, severity, cooldown, deduplication, acknowledgement, and paired recovery.
- Desktop-present macOS operation can deliver through Notification Center; headless operation retains durable alert records.
- An optional command hook has explicit timeout, environment, output, concurrency, and retry bounds.
- Delivery failure is separately observable and never starts, stops, restarts, retries, or cancels authoritative work.

## Scope
### In Scope
- Define bounded metric samples, typed operator events, alert rules, alert instances, acknowledgement, and recovery contracts.
- Persist durable event/alert lifecycle and explicit metric retention or downsampling bounds.
- Add a transport-independent notification port and macOS local delivery adapter.
- Add headless durable records and a bounded optional command hook.
- Expose metric, event, and alert queries through application/shared contracts.
### Out of Scope
- [hard] Do not add SaaS monitoring, email, SMS, or remote webhook delivery.
- [deferred] Full alert/metric presentation and operator workflows belong to `docs/briefs/2026-07-25-feat-macos-prod-08-operator-ui.md`.
- [hard] Do not let hooks inherit undeclared secrets or run without timeout/concurrency/output limits.
- [hard] Do not add new test files or test cases without separate user approval.

## Constraints
- Start after bounded log storage is available at `crates/infra/logging/src/lib.rs`.
- Commit authoritative process/job outcomes before generating or delivering derived operator events.
- Bound every retained record family by count, age, sampling interval, or explicit downsampling policy.
- Persist alert deduplication, acknowledgement, cooldown, and recovery state across daemon restart.
- Notification adapters report delivery outcome only; they never own process or schedule decisions.
- Worker decision: alert rules are persisted CRUD resources with stable IDs exposed through application/shared contracts; transport routes and UI arrive in Wave 8. Config import is deferred and cannot replace operator-edited rules in this child.
- Worker decision: the daemon samples each live instance every `5s` on a monotonic cadence with UTC observation time; aggregate values are derived at query time. Raw samples retain `24h`, one-minute downsampled points retain `30d`, and events/alerts retain at most `100,000` records and `90d`/`365d` respectively.
- Worker decision: authoritative state mutation and observability enqueue share the existing durable outbox transaction or an equivalent reconstructable cursor; post-commit best-effort publish alone is insufficient.
- Worker decision: the daemon exclusively claims delivery candidates with a `60s` renewable lease and stable attempt ID; restart expires/reclaims leases. Alert lifecycle is exactly-once by dedupe key, while delivery attempts are at-least-once and may report duplicates.
- Worker decision: pending/leased candidates cap at `10,000`; completed delivery attempts retain at most `100,000` and `90d`; rules cap at `1,000`, and deleted-rule tombstones retain `365d` so replay cannot resurrect them.
- Worker decision: the daemon owns Notification Center delivery when a GUI session/permission is available; desktop never claims candidates. Unavailable OS delivery leaves durable in-app/headless state.
- Worker decision: hooks use executable/argv without a shell by default, optional explicit working directory, allowlisted environment, process-group TERM/KILL/reap, `30s` timeout, `64KiB` combined output, concurrency `4`, and at most `3` delivery attempts with bounded backoff.
- Worker decision: initial typed conditions are restart count/rate, process-group CPU or memory threshold with consecutive/window semantics, health/readiness state duration, rollout failure, schedule misfire/retry/overflow/final failure, and delivery failure. Each condition declares threshold/window/clear predicate; missing resource samples are unknown and do not breach or recover.
- Worker decision: one-minute UTC buckets store count, average, minimum, maximum, and latest for CPU/memory; health/readiness store latest plus transition count. Missing buckets emit no point, late samples cannot rewrite sealed buckets, and a shutdown-flushed partial bucket is marked partial.
- Worker decision: alert state is `active`, `acknowledged_active`, or `resolved`. Acknowledgement suppresses repeat notification but not evidence or recovery; recovery always resolves/notifies the same episode. A later breach creates a new alert ID. Episode dedupe key is `(rule_id, source_id, normalized_cause, first_breach_boundary)`.
- Worker decision: rules bind one-to-many separate `DeliveryBinding` resources; hook executable/argv/working-dir/environment allowlist lives in the binding, not the rule. Rule deletion tombstones bindings and pending candidates resolve as cancelled rather than executing stale config.
- Worker decision: bounded query ordering is `(occurred_at_utc, stable_id)` with opaque cursor, explicit earliest-retained boundary, and snapshot high-watermark so cleanup cannot create duplicates; expired cursors return retention-boundary error.
- Worker decision: observability migration creates empty additive tables for legacy state, uses foreign keys/indexes on source/rule/state/time, and performs cleanup in bounded transactions isolated from authoritative tables; failure rolls back and restart retries idempotently.
- Worker decision: hook receives a versioned JSON event document on stdin with no argument templating; executable and working directory are absolute, no-follow, user-owned, non-world-writable paths, and environment values are literal.

## Related Files / Entry Points
- `crates/application/src/events.rs` — start with typed production events and alert candidates.
- `crates/application/src/facade.rs` — publish committed outcomes and query bounded history.
- `crates/core/src/ports/mod.rs` — introduce narrow telemetry storage and notification-delivery capabilities.
- `crates/infra/sqlite/src/lib.rs` — persist bounded event/alert state and acknowledgement.
- `crates/daemon/src/lib.rs` — wire headless and desktop-capable macOS delivery adapters.
- `crates/platform/macos/src/lib.rs` — locate the Notification Center adapter behind a platform port.
- `crates/shared/src/api.rs` — expose additive event, metric, alert, and delivery DTOs.
- `docs/evidence/macos-prod-07-telemetry-alerts.md` (proposed) — record crash-boundary, retention, cooldown, recovery, permission, and hook evidence.

## Execution Plan
### Stage 1 — Define bounded operational evidence contracts
- Starts when: Cursor-safe bounded log storage is available at `crates/infra/logging/src/lib.rs`.
- Work: First verify macOS notification feasibility; create a source-transition coverage matrix naming each process guardrail/rollout and schedule retry/overflow/final commit, its transaction/outbox cursor, emitted event identity, and recovery proof; then define typed metrics/events/rules/bindings/alerts/ack/recovery/delivery and retention contracts.
- No-op when: Existing contracts already cover every production event family with durable bounded alert lifecycle and transport-independent delivery.
- No-op handoff: Publish the confirmed contract at `crates/application/src/events.rs` and record retention/authority proof at `docs/evidence/macos-prod-07-telemetry-alerts.md`; if Stage 2/3 behavior and shared API exposure are also proven, hand the completed child to operator UI/parent, otherwise continue at the first unproven stage.
- Deliverable: Production observability vocabulary in `crates/application/src/events.rs`.
- Verify: `bounded observability contract inspection`; Inputs: process guardrail/rollout outcomes, schedule occurrence/retry/overflow outcomes, alert lifecycle, and delivery results; Expected: each event has stable identity, severity/cause, retention ownership, and no authority to mutate supervised state.
- Ends when:
  - [ ] Restart, health/readiness, memory, rollout, misfire, retry, overflow, final failure, recovery, and delivery failure have typed representations.
  - [ ] Equivalent alert identity and cooldown behavior are deterministic.
- Handoff: Stage 2 receives stable persisted record and rule contracts.
- Replan when: Metric write volume cannot satisfy explicit bounds; keep operator events durable and choose bounded in-memory/downsampled metrics with documented resolution loss.

### Stage 2 — Persist and evaluate alerts after state commits
- Starts when: Stage 1 defines stable event identity, retention, and alert lifecycle.
- No-op when: The daemon sampler, durable enqueue, persisted rule CRUD, bounded queries, alert lifecycle, and leased candidates already satisfy Stage 2 verification; record proof and continue to Stage 3 without persistence edits.
- Work: Start one daemon-owned cancellable `5s` per-instance sampler, store/downsample metrics and events, atomically enqueue committed outcomes, expose rule/query/acknowledgement/delivery APIs, evaluate rules, persist dedupe/cooldown/recovery, and lease candidates.
- Deliverable: `DeliveryCandidate` and claim interface in `crates/application/src/events.rs`/core ports with stable candidate, alert, rule, event, source, lease-owner/deadline, attempt-count, delivery-kind, payload, and created-at fields; SQLite implements it and `crates/shared/src/api.rs` exposes bounded rule/metric/event/alert/delivery CRUD/query DTOs.
- Verify: `cargo check --workspace`; Inputs: core ports, application publisher/sampler/evaluator/query CRUD, shared DTOs, SQLite migration/cleanup/lease, daemon worker, and macOS adapter; Expected: exit code `0`, explicit retention for every record family, sampler cancellation ownership, and no delivery call inside authoritative transactions.
- Ends when:
  - [ ] Equivalent events inside cooldown update evidence without notification storms.
  - [ ] Acknowledgement and active/resolved state survive restart.
  - [ ] Cleanup cannot remove authoritative process, job, occurrence, or run state.
- Handoff: Stage 3 receives durable candidates that can be delivered independently.
- Replan when: Transaction ordering can expose an alert for an uncommitted outcome; route evaluation through the existing durable outbox pattern before continuing.

### Stage 3 — Deliver bounded local notifications
- Starts when: Stage 2 produces durable delivery candidates and recovery transitions.
- No-op when: Positive GUI-session Notification Center delivery, unavailable/denied fallback, headless records, hook bounds, restart lease recovery, and authoritative-state isolation are already observed; record proof and hand off to operator UI.
- Work: Implement Notification Center delivery when available, durable headless records, bounded command hooks, retry reporting, and recovery notification pairing.
- Deliverable: Local operations alert pipeline exposed through `crates/application/src/events.rs`.
- Verify: `bounded macOS observability inspection`; Inputs: `cargo test --workspace` plus parent debug controls extended narrowly to seed/list/ack application-level rules/events without public transport routes, covering sampler restart, authoritative-commit crash, lease recovery, cooldown/resolution, retention/downsampling, hook bounds, positive GUI Notification delivery, denial/unavailable fallback, and headless mode; Expected: tests `0`, correct sampler/CRUD DTO lifecycle, no lost/duplicate alert lifecycle, bounded/reaped attempts, one observed OS notification when supported, durable fallback otherwise, and unchanged authority; record commands/platform capability result at `docs/evidence/macos-prod-07-telemetry-alerts.md`.
- Ends when:
  - [ ] Desktop-present and headless modes retain actionable delivery evidence.
  - [ ] Hook timeout, output, environment, concurrency, and retry bounds are enforced.
  - [ ] Notification permission or adapter failure remains visible and retryable without state mutation.
- Handoff: `docs/briefs/2026-07-25-build-macos-prod-09-service-owner.md` receives the daemon-composition observability contract first; after installed-owner authentication/discovery is proven, `docs/briefs/2026-07-25-feat-macos-prod-08-operator-ui.md` consumes the same query and alert contract through that owner.
- Replan when: Notification Center requires unavailable permissions or entitlements; retain durable in-app alerts, mark OS delivery unavailable, and route the entitlement requirement to `docs/briefs/2026-07-25-build-macos-prod-10-release-gates.md`.

## Side Effect Checkpoints
- [ ] Alert delivery failure cannot start, stop, restart, retry, or cancel authoritative work.
- [ ] Metrics/event cleanup cannot delete process definitions, job definitions, occurrences, runs, or log segments.
- [ ] Replaying the durable outbox does not create duplicate active alert instances.
- [ ] Headless daemon operation does not depend on desktop availability.
- [ ] Hook execution exposes only configured environment and remains process-group cancellable.

## Acceptance Criteria
- [ ] Production metrics and operator events are queryable and bounded by explicit retention/downsampling.
- [ ] Equivalent alerts within cooldown produce one active notification lifecycle and recovery produces one paired resolved state.
- [ ] Acknowledgement, cooldown, dedupe, and recovery survive daemon restart.
- [ ] Desktop-present and headless modes retain actionable records when local delivery fails.
- [ ] Alert evaluation and delivery failures leave authoritative process/job outcomes unchanged.

## Open Questions
- None — alerts are local, bounded, durable where required, and downstream of committed state.
