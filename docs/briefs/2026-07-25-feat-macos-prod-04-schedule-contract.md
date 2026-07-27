# [feat] Define deterministic schedule contracts

## Work Type
feat

## Current State (As-Is)
- [confirmed] As of `a1c845c` on `main`, `JobTrigger` supports 5-field cron, fixed interval, UTC one-shot, and dependency triggers — Evidence: `JobTrigger` in `crates/core/src/domain/job.rs`.
- [confirmed] `Job` supports `Skip`, `Queue`, or `Parallel` overlap, dependency-failure behavior, timeout, and run-log retention but no timezone, DST, misfire, retry, concurrency-cap, queue-cap, or rate policy — Evidence: `Job`, `OverlapPolicy`, and `LogRetention` in `crates/core/src/domain/job.rs`.
- [confirmed] `JobRun` records trigger source and timestamps but has no durable occurrence identity or retry attempt relationship — Evidence: `JobRun` and `TriggeredBy` in `crates/core/src/domain/job.rs`.
- [confirmed] Cron calculation uses `DateTime<Utc>` directly, and scheduler fire timestamps are discarded when `run_scheduler_loop()` calls `on_schedule_tick()` by name only — Evidence: `next_cron()` in `crates/infra/scheduler/src/lib.rs` and scheduler select branch in `crates/application/src/facade.rs`.
- [confirmed] Existing job storage serializes the trigger as JSON and stores runs by `run_id`; no persisted schedule cursor or unique logical occurrence exists — Evidence: `SqliteStore::migrate`, `jobs.trigger`, and `job_runs` in `crates/infra/sqlite/src/lib.rs`.
- [inferred] Production catch-up and retry cannot be made idempotent until one logical scheduled occurrence has a durable key distinct from individual attempts — Confirm by: simulating a crash after occurrence claim and before run dispatch against the proposed schema.

## Desired Outcome (To-Be)
- Every cron schedule has an explicit persisted IANA timezone; existing cron definitions retain UTC behavior, while new definitions resolve and persist the current macOS timezone when omitted.
- DST behavior is deterministic: nonexistent local times are skipped and repeated local times produce exactly one occurrence.
- Every timed occurrence has a durable identity, original scheduled time, attempt number, and dispatch state.
- Each job declares a missed-run policy (`skip`, `run_once`, or bounded `catch_up`), retry/backoff policy, maximum concurrency, queue capacity, and overflow behavior.
- Current overlap values remain readable and map deterministically onto the richer execution policy.
- Schedule preview can return upcoming UTC instants and their local-time representations without mutating scheduler state.

## Scope
### In Scope
- Extend domain, persisted, config, shared DTO, and HTTP contracts for production scheduling semantics.
- Define occurrence, attempt, misfire, retry, concurrency, queue, and DST vocabularies.
- Add transactional migrations and compatibility mapping for existing jobs/runs.
- Preserve the scheduled timestamp from scheduler event through persisted run history.
- Add a pure schedule-preview contract suitable for CLI and desktop consumers.
### Out of Scope
- [deferred] Durable occurrence claiming, catch-up execution, retries, and admission control belong to `docs/briefs/2026-07-25-feat-macos-prod-05-durable-dispatch.md`.
- [deferred] Operator forms and preview UI belong to `docs/briefs/2026-07-25-feat-macos-prod-08-operator-ui.md`.
- [hard] Do not silently reinterpret an existing cron expression in the macOS local timezone; migrated jobs preserve the current UTC calculation.
- [hard] Do not add new test files or test cases without separate user approval.

## Constraints
- Use IANA timezone identifiers, store resolved timezone explicitly, and reject unavailable identifiers before persistence.
- Default new cron jobs to the current macOS IANA timezone, default missed-run handling to `run_once`, skip nonexistent DST times, and run repeated local times once.
- Existing `Skip`, `Queue`, and `Parallel` values must deserialize and map to equivalent admission behavior; global production safety limits may bound formerly unbounded `Parallel`.
- Retry attempts belong to one logical occurrence and trigger downstream dependencies only after the occurrence reaches its final terminal outcome.
- Keep `run_id` stable as an attempt identity and add occurrence identity additively.
- Worker decision: repeated DST local time selects the earlier UTC instant; canonical occurrence identity is `(job_id, schedule_revision, trigger_id, scheduled_at_utc)`, and attempts have separate IDs.
- Worker decision: `schedule_revision` is a persisted monotonic `u64`, initialized to `0` for legacy jobs and incremented transactionally only when trigger membership/expression, interval/one-shot time, timezone, or DST interpretation changes. Every trigger has a persisted UUID; migration assigns one once, and replacing a trigger assigns a new UUID while non-temporal policy edits preserve both revision and trigger IDs.
- Worker decision: legacy `Skip` maps to concurrency `1`/queue `0`, `Queue` to concurrency `1`/queue `1024`, and `Parallel` to concurrency `32`/queue `1024`, all under global concurrency `128`; overflow remains explicit/configurable.
- Worker decision: legacy missed-run behavior maps to `skip`; only new jobs default to `run_once`. Queue/Parallel overflow defaults to `reject_new` with a persisted explicit outcome; `Skip` records the skipped overlap. The caps are a documented production-safety exception to formerly unbounded behavior.
- Worker decision: retry defaults to disabled (`max_attempts=1` including initial); enabled retry uses exponential backoff (`1s`, factor `2`, cap `5m`, jitter `20%`) for failed/timed-out attempts, never manual cancellation. Catch-up is oldest-first, capped at `100` and `24h`.
- Worker decision: retry jitter is a uniform offset in `[-20%, +20%]`, calculated once with the runtime random source when an attempt finishes; the resulting absolute `next_attempt_at` is persisted atomically with attempt state and is never recomputed after restart.
- Worker decision: legacy runs retain nullable occurrence/original-scheduled fields instead of fabricated history.
- Worker decision: omitted timezone must resolve and normalize to a canonical macOS IANA ID; failure requires explicit input and never silently falls back to UTC.
- Worker decision: preview requires a reference instant, defaults to `10`, caps at `100`, and stops after `5y` or `100,000` candidates with a stable bounded/unsatisfiable error.
- Worker decision: occurrence states are `claimed`, `queued`, `running`, `retry_waiting`, `succeeded`, `failed`, `timed_out`, `cancelled`, and `skipped`; only the last five are final. Attempt `JobRun` states remain separate. The application runner commits the final occurrence state and dependency outbox atomically; Wave 5 implements these transitions.
- Worker decision: job create/update repository operations compare the persisted temporal fields and increment `schedule_revision` in the same transaction that writes a temporal change; optimistic revision mismatch returns conflict and non-temporal policy edits preserve revision.
- Worker decision: public preview is `POST /v1/jobs/preview`, Tauri `preview_job`, and `msv job preview --config <path> --at <rfc3339> --count <n>` with additive shared request/result/error DTOs.

## Related Files / Entry Points
- `crates/core/src/domain/job.rs` — start with deterministic schedule, occurrence, and execution-policy vocabulary.
- `crates/core/src/ports/scheduler.rs` — carry full scheduled occurrence metadata across the scheduler boundary.
- `crates/shared/src/api.rs` — expose additive job, run, preview, and policy DTOs.
- `crates/config/src/convert.rs` — apply compatibility and new-job timezone defaults.
- `crates/infra/scheduler/src/lib.rs` — calculate timezone-aware occurrences without mutating dispatch state.
- `crates/infra/sqlite/src/repr.rs` — version the local trigger representation.
- `crates/infra/sqlite/src/lib.rs` — migrate job definitions, run occurrences, and schedule cursor state atomically.
- `crates/infra/http/src/mapping.rs` — preserve every policy and timestamp through wire conversion.
- `crates/application/src/facade.rs` — propagate scheduler occurrence/original time into final `JobRun`, own revisioned create/update and pure preview use cases.
- `crates/infra/http/src/lib.rs` — register the preview route.
- `crates/infra/http/src/handlers.rs` — call the pure preview application operation.
- `crates/cli/src/client.rs` — consume preview and schedule DTOs.
- `crates/cli/src/main.rs` — expose the bounded preview command.
- `docs/API.md` — document canonical schedule fields, compatibility exceptions, state transitions, and preview contract.
- `docs/evidence/macos-prod-04-schedule-contract.md` (proposed) — record migration, fixed-clock DST, occurrence-key, and preview evidence.

## Execution Plan
### Stage 1 — Stabilize temporal and admission vocabulary
- Starts when: Current job and scheduler contracts in `crates/core/src/domain/job.rs` and `crates/core/src/ports/scheduler.rs` are confirmed.
- Work: Define timezone, DST, missed-run, retry, concurrency, queue, occurrence, and preview contracts with backward-compatible mappings.
- No-op when: All named policies and occurrence metadata already exist and current cron values have an explicit compatibility timezone.
- No-op handoff: Parent and durable-dispatch brief receive the confirmed contract at `crates/core/src/domain/job.rs`; continue only after all policies and defaults are present.
- Deliverable: Deterministic schedule contract in `crates/core/src/domain/job.rs`.
- Verify: `cargo check -p my-supervisor-core`; Inputs: job domain and scheduler port; Expected: exit code `0` and the scheduler event carries occurrence identity plus original scheduled time.
- Ends when:
  - [ ] Local-time ambiguity and nonexistence have explicit outcomes.
  - [ ] One logical occurrence and its attempts cannot be confused.
  - [ ] Current overlap policies have documented compatibility mappings.
- Handoff: Stage 2 receives the stable job and occurrence vocabulary.
- Replan when: The timezone library in the current dependency graph cannot resolve IANA zones or DST transitions; select and justify the narrowest compatible dependency before continuing.

### Stage 2 — Migrate durable job and run state
- Starts when: Stage 1 fixes the domain fields and compatibility defaults.
- No-op when: A baseline `a1c845c` worktree fixture already migrates/reopens with legacy UTC/run identity intact, clean foreign keys, duplicate occurrence rejection, and complete all-fields round trips; record proof and continue to Stage 3.
- Work: Add atomic schema and representation migrations for policies, explicit timezone, occurrence identity, attempts, and schedule cursor state.
- Deliverable: Reopened SQLite state preserves old jobs/runs and round-trips all new schedule fields.
- Verify: `isolated schedule migration round-trip inspection`; Inputs: `cargo check -p my-supervisor-infra-sqlite` and the parent baseline-worktree/debug-daemon procedure to create cron/interval/one-shot/dependency jobs and completed runs at `a1c845c`, then candidate reopen, `PRAGMA foreign_key_check`, repeated occurrence insertion, and all-fields round trip; Expected: exit code `0`, empty foreign keys, old UTC/run IDs preserved, duplicate key rejected, and policies/attempt/cursor round-trip; record commands and SQL observations at `docs/evidence/macos-prod-04-schedule-contract.md`.
- Ends when:
  - [ ] Existing cron jobs remain UTC and existing run IDs remain addressable.
  - [ ] New occurrences have a uniqueness boundary that prevents duplicate claims after restart.
  - [ ] Retry attempts retain one occurrence relationship.
- Handoff: Stage 3 receives the durable schedule contract.
- Replan when: Existing duplicate or malformed rows prevent a safe unique occurrence migration; stop and define a deterministic quarantine/repair rule before committing.

### Stage 3 — Propagate DTOs and pure preview
- Starts when: Domain and storage contracts round-trip.
- No-op when: Public mappings and preview already satisfy every fixed-clock/compatibility/bound case without writes or timer changes; record proof and hand the contract to durable dispatch without edits.
- Work: Update config, shared DTOs, revisioned application create/update, scheduler-to-`JobRun` propagation, HTTP/Tauri/CLI mappings, documented occurrence transitions, and pure preview route/output while retaining old request compatibility.
- Deliverable: Public schedule contract ready for durable dispatch at `crates/core/src/domain/job.rs`.
- Verify: `bounded schedule contract propagation inspection`; Inputs: `cargo check --workspace`, parent debug-daemon/CLI commands, and the public preview operation with explicit reference instants for normal/nonexistent/repeated DST, old UTC, revision change, retry, legacy run, timezone-resolution failure, admission mappings, and count/horizon bounds; capture config, DB, API, CLI JSON, scheduler event/outbox counts before/after; Expected: exit code `0`, canonical key/time and earlier-offset rule hold, legacy truth remains, bounds/errors are stable, and preview changes no row/timer/event count; record at `docs/evidence/macos-prod-04-schedule-contract.md`.
- Ends when:
  - [ ] Every boundary preserves timezone, policy, occurrence, attempt, and original schedule values.
  - [ ] Preview returns deterministic upcoming instants without registering a timer or writing a row.
- Handoff: `docs/briefs/2026-07-25-feat-macos-prod-05-durable-dispatch.md` and the parent receive the contract at `crates/core/src/domain/job.rs`.
- Replan when: An existing DTO cannot be extended without breaking deserialization; introduce a versioned additive endpoint or field and keep the prior contract available.

## Side Effect Checkpoints
- [ ] Existing 5-field cron expressions, interval values, one-shot timestamps, and dependency lists remain readable.
- [ ] Existing `TriggeredBy::Schedule`, `Manual`, and `Dependency` run history remains decodable.
- [ ] Existing `Skip`, `Queue`, and `Parallel` values retain their documented behavior mapping.
- [ ] Job deletion, log retention, dependency signatures, transient cleanup, and terminal outbox foreign keys remain valid.
- [ ] Schedule preview is pure and cannot arm, unregister, or dispatch a job.

## Acceptance Criteria
- [ ] Existing persisted cron jobs calculate the same next UTC instant after migration.
- [ ] A new cron job without a timezone stores the current macOS IANA timezone explicitly.
- [ ] DST nonexistent local times yield zero occurrences and repeated local times yield exactly one occurrence.
- [ ] Every scheduled run preserves the scheduler-provided original scheduled time and a unique occurrence ID.
- [ ] `cargo check --workspace` exits `0` with all config, persistence, HTTP, CLI, Tauri, and desktop consumers aligned.

## Open Questions
- None — production defaults use explicit macOS timezone, one-run misfire recovery, and duplicate-safe DST behavior.
