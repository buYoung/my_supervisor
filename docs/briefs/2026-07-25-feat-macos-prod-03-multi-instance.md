# [feat] Add multi-instance rolling supervision

## Work Type
feat

## Current State (As-Is)
- [confirmed] As of `a1c845c` on `main`, `OperationsFacade.runtime` maps one process name to one `RuntimeEntry` and every start/stop/restart path locks and mutates by process name — Evidence: `OperationsFacade`, `process_lock()`, and process methods in `crates/application/src/facade.rs`.
- [confirmed] SQLite stores one runtime handle on `process_specs`, so it cannot represent multiple durable Direct children for one logical process — Evidence: `process_specs.runtime_*` columns in `SqliteStore::migrate`.
- [confirmed] Process APIs and desktop rows expose one PID and one aggregate state — Evidence: `ProcessStatus`, `process_status_to_dto()`, and `ProcessesView`.
- [confirmed] The platform adapter can safely spawn and stop multiple independently owned process groups, but the application layer currently gives each logical process only one active slot — Evidence: `MacLifecycle` child ownership and the single `RuntimeEntry` map.
- [inferred] Generic zero-downtime replacement is only honest when a replacement instance becomes ready before an old instance is drained and when at least one routable instance remains — Confirm by: running a two-instance command with a configured readiness check and observing generation overlap.

## Desired Outcome (To-Be)
- One logical process maintains a configured number of stable instances with independent instance IDs, PIDs, generations, health, readiness, and restart counters.
- Reconciliation repairs missing or excess instances without duplicating live generations after daemon restart.
- Group start, stop, restart, delete, scale, and status operations are deterministic and expose per-instance results.
- Rolling restart starts and verifies replacement capacity before draining old instances; it fails closed when readiness or minimum-healthy guarantees cannot be met.
- Instance environment exposes stable identity and ordinal values so commands can bind distinct ports or resources without Node.js-specific behavior.

## Scope
### In Scope
- Replace the one-entry runtime assumption with group and instance ownership.
- Persist desired/actual instance identity and recover valid detached instances.
- Add scale and rolling-restart application operations with partial-failure reporting and compensation.
- Extend HTTP, Tauri, and CLI contracts for instance detail and group operations.
- Enforce readiness and minimum-healthy gates during rolling replacement.
### Out of Scope
- [hard] Do not implement Node.js cluster internals or assume Node IPC.
- [hard] Do not claim zero downtime for a single unroutable instance without a readiness contract and capacity overlap.
- [deferred] A built-in HTTP/TCP load balancer is not required; applications may use instance environment, `SO_REUSEPORT`, or an external router.
- [deferred] Desktop workflows belong to `docs/briefs/2026-07-25-feat-macos-prod-08-operator-ui.md`.
- [hard] Do not add new test files or test cases without separate user approval.

## Constraints
- Start from the process contract at `crates/core/src/domain/process.rs` and runtime guardrails at `crates/application/src/facade.rs`.
- Preserve process-group and native-generation identity checks for every instance.
- Keep the logical process name as the stable public key; add instance IDs rather than encoding ordinals into the name contract.
- Provide stable `MSV_INSTANCE_ID` and `MSV_INSTANCE_INDEX` environment values without overwriting caller-provided unrelated environment options.
- Any partial rolling failure leaves the last known healthy capacity running and reports the exact failed stage.
- Worker decision: SystemRegistered remains single-instance and rejects scale/rolling operations; no implicit launchd unit set is created.
- Worker decision: rolling defaults to `max_surge=1`, `max_unavailable=0`, with `min_healthy=instances-max_unavailable`. Readiness is mandatory; a zero-downtime claim additionally requires explicit `routability=shared_or_external`.
- Worker decision: ordinals are stable only for a logical slot lifetime; scale-down retires the highest ordinal and ordinal reuse gets a new UUID. Environment exposes UUID and ordinal.
- Worker decision: one group mutation lock orders delete, stop, config replacement, rolling/restart, then scale; request IDs make retries idempotent and conflicts return stable busy/superseded results.
- Worker decision: persist rollout target revision, phase, batch, old/new IDs, deadline, and compensation state so restart resumes or fails closed.
- Worker decision: migrate existing `runtime_*` transactionally to one ordinal-0 instance with a new persisted UUID and existing native generation; a marker detects partial conversion.
- Worker decision: add `GET /v1/processes/{name}/instances`, `POST /v1/processes/{name}/scale`, and `POST /v1/processes/{name}/rolling-restart`; matching Tauri commands are `process_instances`, `scale_process`, and `rolling_restart_process`, and CLI forms are `msv instances <name>`, `msv scale <name> --instances <n>`, and `msv restart <name> --rolling`. Mutation DTOs carry `operation_id`; HTTP also accepts `Idempotency-Key`. Busy/superseded is HTTP `409` and CLI partial exit `3`.
- Worker decision: legacy group state is `running` while any owned instance is active, `crashed` when desired capacity is nonzero and none survives a terminal failure, and `stopped` only for explicit desired-zero/manual stop. Additive readiness/health counts and rollout phase carry degraded/transitional meaning; aggregate operation result is success only when all complete, partial for mixed outcomes, and failed when none complete.

## Related Files / Entry Points
- `crates/application/src/facade.rs` — start at `RuntimeEntry`, runtime maps, process locks, bootstrap, and reconciliation.
- `crates/core/src/domain/process.rs` — consume the group/instance and reload contracts.
- `crates/core/src/ports/repository.rs` — persist instance rows and identity-checked cleanup.
- `crates/infra/sqlite/src/lib.rs` — move from one runtime handle per definition to durable instance ownership.
- `crates/shared/src/api.rs` — expose additive instance details and group-operation results.
- `crates/infra/http/src/handlers.rs` — route scale, instance inspection, and rolling restart.
- `crates/cli/src/main.rs` — expose scriptable group commands and deterministic exit behavior.
- `docs/evidence/macos-prod-03-multi-instance.md` (proposed) — record migration, group-operation, crash-recovery, and rollout evidence.

## Execution Plan
### Stage 1 — Establish group and instance recovery
- Starts when: The process contract exists at `crates/core/src/domain/process.rs` and guardrail ownership exists at `crates/application/src/facade.rs`.
- Work: Replace name-to-single-runtime assumptions with group-to-instance ownership and reconcile durable instance identities at bootstrap.
- No-op when: Runtime, persistence, status, and cleanup already address every child by stable instance ID and generation.
- No-op handoff: Publish confirmed ownership at `crates/application/src/facade.rs` and record identity/recovery proof at `docs/evidence/macos-prod-03-multi-instance.md`; if Stage 2/3 acceptance is also proven, hand the completed child to parent/logging, otherwise continue to the first unproven stage. Any sibling-targeting risk fails the proof.
- Deliverable: Instance-aware runtime and recovery ownership in `crates/application/src/facade.rs`.
- Verify: `cargo check -p my-supervisor-application -p my-supervisor-infra-sqlite`; Inputs: runtime maps, repository instance operations, and bootstrap reconciliation; Expected: exit code `0` and no process-name-only cleanup remains for instance-owned handles.
- Ends when:
  - [ ] Desired and actual instances can be listed and reconciled independently.
  - [ ] Reopening the database cannot duplicate a verified live detached instance.
- Handoff: Stage 2 receives instance-aware lifecycle primitives.
- Replan when: Instance persistence cannot migrate atomically from old `runtime_*` columns; stop and return to the contract migration before group orchestration.

### Stage 2 — Add scale and group lifecycle operations
- Starts when: Stage 1 can create, identify, recover, and remove one instance without affecting siblings.
- No-op when: The declared routes, commands, idempotency, mutation priority, aggregate/per-instance DTOs, and partial exit semantics already satisfy the capability inspection; record proof and continue to Stage 3.
- Work: Implement serialized/idempotent scale and group actions plus per-instance `completed`, `failed`, `not_attempted`, and `superseded` results with stable failure-stage/retryable fields and deterministic CLI partial-failure behavior.
- Deliverable: Scriptable multi-instance lifecycle behavior.
- Verify: `cargo check --workspace`; Inputs: application methods, HTTP handlers, shared DTOs, Tauri invoke mapping, and CLI client/dispatch; Expected: exit code `0` and every operation returns aggregate plus per-instance outcomes.
- Ends when:
  - [ ] Scaling up creates only the missing instances with stable identity environment.
  - [ ] Scaling down drains selected instances and clears only matching durable handles.
  - [ ] Group failures identify completed, failed, and untouched instances.
- Handoff: Stage 3 receives stable scale and group lifecycle operations.
- Replan when: A public route or DTO must remove or reinterpret an existing field; stop and design an additive compatibility response.

### Stage 3 — Enforce readiness-gated rolling replacement
- Starts when: Group operations and runtime guardrail readiness are available.
- No-op when: Persisted rollout phases already resume/fail closed across crash/cancellation and readiness/routability rules preserve healthy capacity; record proof and hand off without rollout edits.
- Work: Replace instances in bounded batches while maintaining configured healthy capacity and compensating safely on failure.
- Deliverable: Production rolling restart behavior and evidence in `crates/application/src/facade.rs`.
- Verify: `bounded macOS multi-instance execution`; Inputs: parent debug harness with a temporary executable that reports instance env/readiness and can fail readiness, plus baseline DB, concurrent/idempotent CLI operations, crashes after spawn/during drain, cancellation, scale-down, and restart; Expected: exact desired count, stable UUID/ordinal rules, readiness before drain, deterministic priority/DTO/exit results, resumed or failed-closed rollout, no orphan/stale handle, and truthful aggregate state; record commands, API snapshots, PIDs/generations, SQLite phases, and cleanup at `docs/evidence/macos-prod-03-multi-instance.md`.
- Ends when:
  - [ ] No old instance drains before replacement readiness and minimum healthy capacity are satisfied.
  - [ ] Cancellation or daemon shutdown stops the rollout without abandoning unowned children.
  - [ ] Single-instance zero-downtime requests fail clearly when overlap cannot be routed safely.
- Handoff: `docs/briefs/2026-07-25-feat-macos-prod-06-log-retention.md`, later observability/UI work, and the parent receive stable multi-instance behavior at `crates/application/src/facade.rs`.
- Replan when: The target command cannot run overlapping generations or readiness cannot prove routability; keep ordinary restart available and reject zero-downtime mode for that definition.

## Side Effect Checkpoints
- [ ] Existing single-instance start, stop, restart, remove, convert, and config reload semantics remain valid.
- [ ] Restart counters and pending restart tokens are isolated per instance where required and aggregate correctly at process level.
- [ ] Detached process recovery compares instance ID, process ID, PID, process group, and generation.
- [ ] SystemRegistered definitions do not silently create multiple launchd units unless the contract explicitly supports them.
- [ ] Bulk operations never overwrite caller environment except the documented `MSV_INSTANCE_*` keys.

## Acceptance Criteria
- [ ] A process configured with `instances = N` converges to exactly `N` owned live instances after start and after daemon restart.
- [ ] Scale-up, scale-down, stop, restart, and delete report per-instance outcomes and leave no stale durable handles.
- [ ] A successful rolling restart preserves configured minimum healthy capacity until all generations are replaced.
- [ ] A readiness failure retains old healthy capacity and returns a recoverable failed-stage result.
- [ ] Existing single-instance clients remain functional through additive aggregate/per-instance DTOs.
- [ ] Instance environment identity/ordinal, idempotent request replay, mutation priority, legacy migration marker recovery, aggregate-state rules, and persisted rollout crash recovery match the declared worker decisions.

## Open Questions
- None — production scope requires generic multi-instance supervision without Node-specific cluster coupling.
