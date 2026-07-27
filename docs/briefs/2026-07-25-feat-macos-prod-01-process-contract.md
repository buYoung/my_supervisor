# [feat] Define production process contracts

## Work Type
feat

## Current State (As-Is)
- [confirmed] As of `a1c845c` on `main` with a pre-existing dirty worktree, `ProcessSpec` represents one command with one runtime identity and no instance, watch, resource-limit, health-check, or reload policy — Evidence: `ProcessSpec`, `ChildHandle`, and `ProcessStatus` in `crates/core/src/domain/process.rs`.
- [confirmed] Restart behavior supports enablement, retry limits, exponential backoff, jitter, and stable-run reset; graceful shutdown supports one signal and grace period — Evidence: `RestartPolicy` and `ShutdownPolicy` in `crates/core/src/domain/process.rs`.
- [confirmed] SQLite stores one optional runtime handle directly on each `process_specs` row and has no process-instance table — Evidence: `SqliteStore::migrate` and the `runtime_*` columns in `crates/infra/sqlite/src/lib.rs`.
- [confirmed] HTTP/config DTO mappings expose current restart and shutdown fields but do not carry production supervision policies — Evidence: `restart_policy()`, `shutdown_policy()`, and `process_config_to_spec()` in `crates/infra/http/src/mapping.rs`.
- [confirmed] Process status already exposes PID, restart count, CPU percentage, and memory bytes, so the new contract must extend rather than replace the existing operator shape — Evidence: `ProcessStatus` in `crates/core/src/domain/process.rs` and `process_status_to_dto()` in `crates/infra/http/src/mapping.rs`.
- [inferred] Adding runtime behaviors before stabilizing additive persisted and wire contracts would force repeated cross-crate edits and make old configuration/database compatibility ambiguous — Confirm by: compiling all workspace consumers after introducing the contract without enabling new behavior.

## Desired Outcome (To-Be)
- A process definition can declare an instance count, per-instance identity/environment contract, file-watch restart policy, memory ceiling policy, liveness/readiness checks, and rolling-reload policy.
- Existing single-instance process definitions and stored rows deserialize to behavior-equivalent defaults without manual migration.
- Process status can report aggregate group state and per-instance state while preserving the current single-process fields for existing clients.
- Config, SQLite, domain, HTTP, CLI-facing DTO, and desktop wire contracts use one vocabulary and one unit convention for every new field.
- Invalid combinations, including zero instances, zero-duration thresholds, and zero-downtime reload without a readiness contract, fail before persistence or process mutation.

## Scope
### In Scope
- Extend process domain types with production supervision and instance contracts.
- Define additive process status and event identity for group and instance observations.
- Add SQLite migration and round-trip support for new definition fields and durable instance identities.
- Update shared DTOs, TOML conversion, HTTP mapping, and documented API/config shapes.
- Preserve old stored rows, old TOML files, and current API consumers through defaults and optional additive fields.
### Out of Scope
- [deferred] Runtime enforcement of file, memory, and health policies belongs to `docs/briefs/2026-07-25-feat-macos-prod-02-runtime-guards.md`.
- [deferred] Multi-instance reconciliation and rolling restart execution belongs to `docs/briefs/2026-07-25-feat-macos-prod-03-multi-instance.md`.
- [hard] Do not add Node.js cluster APIs or embed a Node-specific load balancer; the contract must apply to arbitrary executables.
- [hard] Do not add new test files or test cases without separate user approval.

## Constraints
- The current working tree is the implementation source of truth and all pre-existing edits are preserved; `a1c845c` is used only to manufacture legacy migration fixtures and must never overwrite current files.
- Keep `ProcessSpec.name`, `ManagementMode`, `LifecycleMode`, existing restart fields, shutdown fields, API route names, and existing event names backward compatible.
- Use explicit units in field names: bytes for memory and milliseconds or seconds consistently across each public boundary.
- Default all new disruptive behaviors off; default `instances` to `1`.
- Represent instance identity independently from OS PID so PID reuse cannot attach status or cleanup to a replacement instance.
- Existing rows with one `runtime_*` handle must migrate atomically or remain readable through a compatibility branch until migration completes.
- Worker decision: `instances` is configurable from `1` through `128`; watch roots are explicit and capped at `256`. Enabled checks default to `10s` interval, `3s` timeout, and `3` consecutive liveness failures; rolling defaults are `max_surge=1`, `max_unavailable=0`, and `readiness_timeout=60s`.
- Worker decision: memory limits are disabled by default and, when enabled, use aggregate resident bytes for the owned process group including descendants, sampled every `5s` with `3` consecutive breaches.
- Worker decision: Direct-only watch, memory, multi-instance, and rolling policies on SystemRegistered definitions fail validation; launchd retains restart ownership.
- Worker decision: for multi-instance aggregate status, legacy `pid` is `null`, `started_at` is the oldest active start, and restart/CPU/memory are sums; the additive per-instance list carries exact values. Single-instance semantics remain unchanged.
- Worker decision: one core validation constructor is authoritative and config/HTTP/Tauri/CLI map its stable codes; absent fields select defaults, `null` is accepted only for optional observations, and unknown fields follow current deserializer behavior.
- Worker decision: a persisted UUID identifies a logical slot across restarts, generation identifies each spawn, scale-down retires the highest ordinal, ordinal reuse receives a new UUID, and deleted IDs are never reused.
- Worker decision: watch contract fields are roots, recursive flag, exclusions, `follow_symlinks=false`, and `debounce_ms`; health/readiness support explicit executable/argv, TCP connect, or HTTP GET with interval, timeout, success criteria, and consecutive thresholds. `MSV_INSTANCE_ID`/`MSV_INSTANCE_INDEX` are reserved supervisor keys that override only caller values with those exact names.
- Worker decision: represent policies as additive serde structs: `instances: NonZeroU16`, optional `WatchPolicy`, optional `MemoryPolicy { ceiling_bytes: NonZeroU64, sample_interval_ms, consecutive_breaches }`, optional `CheckPolicy { kind: exec|tcp|http, interval_ms, timeout_ms, consecutive_successes, consecutive_failures }`, and optional `RollingPolicy { max_surge, max_unavailable, readiness_timeout_ms, routability }`; omitted/null optional policies are disabled.
- Worker decision: validation rejects empty/duplicate/non-absolute roots, symlink following, zero debounce/interval/timeout/thresholds, timeout not below interval, memory sampling outside bounded limits, `max_unavailable >= instances`, zero overlap capacity, invalid check target/status range, and caller attempts to define reserved instance environment keys.
- Worker decision: SQLite adds normalized `process_instances(process_id, instance_id, ordinal, generation, pid, pgid, started_at, state, ...)` with unique process/instance and process/ordinal keys; complex definition policies use versioned JSON representation columns with `{}`/null compatibility defaults. Existing `runtime_*` values migrate transactionally into ordinal 0.
- Worker decision: event identity is a durable UUID `event_id` plus process/instance/generation, using the workspace `uuid` dependency; ordering belongs to the later event/outbox contract rather than a new `event_sequence` here.

## Related Files / Entry Points
- `crates/core/src/domain/process.rs` — start with the stable domain vocabulary and backward-compatible defaults.
- `crates/core/src/ports/repository.rs` — extend persistence operations for instance identity without leaking SQLite types.
- `crates/shared/src/api.rs` — add optional wire fields and aggregate/per-instance status shapes.
- `crates/config/src/convert.rs` — map TOML DTOs into the same domain defaults.
- `crates/infra/http/src/mapping.rs` — keep request and response mappings symmetric.
- `crates/infra/sqlite/src/lib.rs` — add transactional schema migration and read/write round trips.
- `docs/API.md` — document the additive contract and compatibility behavior.
- `crates/application/src/events.rs` — carry durable `event_id` plus process/instance/generation for process observations without making events authoritative.
- `crates/desktop/src/main.rs` — preserve additive Tauri command/result mappings to the shared facade.
- `crates/cli/src/client.rs` — preserve process DTO/error decoding.
- `crates/cli/src/main.rs` — preserve existing process output/exit behavior while exposing additive detail.
- `docs/evidence/macos-prod-01-process-contract.md` (proposed) — record migration, round-trip, invalid-input, and rollback evidence.

## Execution Plan
### Stage 1 — Stabilize domain and compatibility vocabulary
- Starts when: The confirmed single-instance baseline in `crates/core/src/domain/process.rs` is available.
- Work: Define instance, watch, memory, health, and reload policy types, then implement one authoritative constructor/validator with stable error codes before any config, repository, or transport mapping consumes the values.
- No-op when: `ProcessSpec` already carries all named policies, `instances` defaults to `1`, and old serialized values load without behavior change.
- No-op handoff: Parent and successor briefs receive the confirmed contract at `crates/core/src/domain/process.rs`; continue only after the bounded inspection proves every required field and default.
- Deliverable: Additive process contract in `crates/core/src/domain/process.rs` with stable group and instance identities.
- Verify: `bounded process contract inspection`; Inputs: `cargo check -p my-supervisor-core`, old serialized definitions, all default/limit boundaries, Direct/SystemRegistered combinations, and the parent debug-harness procedure; Expected: exit code `0`, old definitions select identical behavior, every invalid combination returns the same stable code before mutation, and evidence is recorded at `docs/evidence/macos-prod-01-process-contract.md`.
- Ends when:
  - [ ] Each production policy has one domain type, explicit units, validation boundaries, and a backward-compatible default.
  - [ ] Group identity, instance identity, PID, process generation, and operator-visible state are unambiguous.
- Handoff: Stage 2 receives the domain contract and exact defaults.
- Replan when: A required policy cannot be represented additively without changing existing enum variants or default semantics; stop and return to the parent for a compatibility decision.

### Stage 2 — Persist definitions and instance identities
- Starts when: Stage 1 provides the stable process contract.
- No-op when: The baseline worktree fixture already migrates/reopens atomically, old handles/counters are intact, all new fields round-trip, and foreign keys are clean; record proof and continue to Stage 3 without schema edits.
- Work: Add an atomic SQLite migration and repository round trips for new definition fields and durable per-instance identities.
- Deliverable: A reopened database preserves old process rows and the complete new process contract.
- Verify: `isolated process migration round-trip inspection`; Inputs: `cargo check -p my-supervisor-infra-sqlite`, the parent `a1c845c` worktree fixture, and candidate termination before transaction, during staged writes, after commit-before-reopen, plus repeated reopen, `PRAGMA foreign_key_check`, and all-fields CRUD; Expected: exit code `0`, rollback preserves original rows at pre-commit failures, committed migration is idempotent, old handle/counter becomes exactly one instance, all fields round-trip, and delete leaves no orphan; record at `docs/evidence/macos-prod-01-process-contract.md`.
- Ends when:
  - [ ] Migration, write, read, update, delete, and reopen paths agree on every new field.
  - [ ] Instance cleanup compares stable identity and generation before deleting a durable handle.
- Handoff: Stage 3 receives the persisted contract and migration behavior.
- Replan when: The existing inline migration cannot atomically preserve old runtime handles; stop dependent work and design a staged compatibility migration before continuing.

### Stage 3 — Propagate the contract to every public boundary
- Starts when: The domain and persistence round trip is stable.
- No-op when: The old-minimal and all-fields definitions already traverse config, domain, reopened SQLite, HTTP, status, event, and Tauri mappings with identical units/defaults/errors and rollback proof; record evidence and hand off without mapping edits.
- Work: Extend shared DTOs, TOML conversion, HTTP/status/event/Tauri mapping, CLI client/output, and API documentation without removing old fields; include stable process/instance/generation identity plus durable UUID `event_id`, leave event ordering to the later event/outbox contract, and propagate authoritative validation-error mapping.
- Deliverable: Compile-time-aligned public process contract ready for runtime and UI consumers.
- Verify: `bounded process contract propagation inspection`; Inputs: `cargo check --workspace` plus one non-default definition passed TOML → domain → SQLite reopen → HTTP → status/event/Tauri → CLI JSON/table, an old minimal definition, invalid combinations, and failed config reload; Expected: exit code `0`, values/units/identities/errors agree, old defaults/aggregate semantics and CLI behavior hold, and rollback restores every prior field; record at `docs/evidence/macos-prod-01-process-contract.md`.
- Ends when:
  - [ ] Config, API requests, API responses, Tauri mapping inputs, and persisted values use the same field names and units.
  - [ ] Existing clients can continue reading the old status fields for single-instance processes.
- Handoff: `docs/briefs/2026-07-25-feat-macos-prod-02-runtime-guards.md`, `docs/briefs/2026-07-25-feat-macos-prod-03-multi-instance.md`, and the parent receive the contract at `crates/core/src/domain/process.rs`.
- Replan when: An additive response cannot preserve an existing client assumption; stop and surface the exact wire compatibility break to the parent.

## Side Effect Checkpoints
- [ ] Existing Direct and SystemRegistered config values deserialize with `instances = 1` and all new policies disabled.
- [ ] Existing `restart_count`, `runtime_process_id`, `runtime_pid`, `runtime_pgid`, `runtime_generation`, and `runtime_started_at` data remain readable.
- [ ] `ProcessStatus.name`, `state`, `pid`, `restart_count`, `started_at`, `cpu_percent`, and `memory_bytes` retain their current types.
- [ ] `ManagementMode::SystemRegistered { unit_name }` remains representable without Direct-only policy enforcement.
- [ ] Config reload rollback snapshots include every new field and restore the old target atomically.

## Acceptance Criteria
- [ ] `cargo check --workspace` exits `0` after all domain, storage, config, and wire consumers are updated.
- [ ] Opening an existing database produces one logical instance per existing process without losing its durable handle or restart counter.
- [ ] A new definition round-trips all production fields through TOML, domain, SQLite, HTTP DTO, and status DTO without unit conversion drift.
- [ ] Invalid instance counts and invalid policy durations are rejected before any row, timer, process, or launchd unit is changed.
- [ ] The contract is sufficient for both runtime-policy and multi-instance briefs to start without redefining process fields.

## Open Questions
- None — production scope and compatibility defaults are fixed by the user goal and existing contracts.
