# [build] Install one production macOS service owner

## Work Type
build

## Current State (As-Is)
- [confirmed] As of `a1c845c` on `main`, daemon runtime supports macOS adapters, binds loopback by default, and desktop embeds the same `OperationsFacade` in-process — Evidence: `build_runtime()`, `platform_adapters()`, and constants in `crates/daemon/src/lib.rs`, plus desktop runtime assembly.
- [confirmed] Managed SystemRegistered processes receive per-process user LaunchAgent plists, but the supervisor daemon itself has no install/uninstall/status lifecycle — Evidence: `LaunchdAgentProcess` in `crates/platform/macos/src/launchd.rs`, `Command`/`DaemonCmd` in `crates/cli/src/main.rs`, and route search in `crates/infra/http/src/lib.rs`.
- [confirmed] `LaunchdAgentProcess::new()` falls back to `/tmp` when `HOME` is missing, and daemon path helpers retain fallback locations rather than one explicit production failure contract — Evidence: constructors in `crates/platform/macos/src/launchd.rs` and `crates/daemon/src/lib.rs`.
- [confirmed] Desktop and standalone daemon can assemble runtimes over the same default storage paths; no process-wide data-directory ownership lock is visible — Evidence: `build_deps_with_helpers_and_store()` in `crates/daemon/src/lib.rs` and runtime assembly in `crates/desktop/src/main.rs`.
- [inferred] Concurrent installed-daemon and embedded-desktop owners could create duplicate scheduler loops and process supervisors over one SQLite/log state — Confirm by: launch both hosts against one isolated data directory and inspect ownership before either recovery loop starts.

## Desired Outcome (To-Be)
- Exactly one macOS user-scoped daemon owns a data directory, migrations, scheduling, and child supervision across login, desktop quit, daemon restart, and upgrade.
- Desktop connects as a client to the installed owner and cannot start a competing embedded owner.
- CLI provides idempotent install, uninstall, start, stop, status, backup, upgrade handoff, rollback, and recovery for the supervisor LaunchAgent.
- Data, config, log, binary, helper, and LaunchAgent paths are stable and fail explicitly instead of using unsafe `/tmp` or current-directory fallbacks.
- Uninstall removes service registration separately from optional data removal and never deletes user data implicitly.

## Scope
### In Scope
- Add a data-directory ownership lock acquired before migration and recovery.
- Add installed-daemon discovery and explicit desktop client/embedded-development behavior.
- Implement user LaunchAgent install/uninstall/start/stop/status and idempotent recovery.
- Define stable versioned binary/helper layout, backup ownership, atomic upgrade handoff, and rollback metadata.
- Harden user-home/data/config/log path resolution and permissions.
### Out of Scope
- [hard] Do not create system-domain daemons, root-owned files, or privileged helpers.
- [deferred] Bundle packaging, CSP, entitlements, signing/notarization inputs, CI release gates, and final release evidence belong to `docs/briefs/2026-07-25-build-macos-prod-10-release-gates.md`.
- [hard] Do not publish, sign, notarize, deploy, or delete user data.
- [hard] Do not add new test files or test cases without separate user approval.

## Constraints
- Start from the completed backend process, schedule, log, telemetry, and alert contracts; this child establishes the installed-owner authentication/discovery/native proxy foundation before the operator-surface child consumes it.
- Acquire exclusive ownership before migrations, scheduler registration, process recovery, or journal mutation.
- Stay in the current macOS user `gui/<uid>` launchd domain and preserve loopback-only control.
- Upgrade must preserve SQLite rows, config journals, logs, process/occurrence identities, and a runnable rollback target.
- Embedded desktop ownership is an explicit development fallback, never an automatic competitor to an installed service.
- Worker decision: canonical root is `~/Library/Application Support/com.my-supervisor/` (`0700`) with `data/`, `config/`, `logs/`, `run/`, `backups/`, and `versions/<version>/bin/`; state/credential files are `0600`. The supervisor plist is `~/Library/LaunchAgents/com.my-supervisor.daemon.plist`. Reject symlinked state roots; only the owned active-version pointer may be a symlink.
- Worker decision: migrate a legacy root by quiescing the sole owner, staging a copy, validating SQLite/config/log manifests, atomically selecting it, and retaining the old root for rollback; never merge live roots.
- Worker decision: use an advisory `fcntl` write lock on `run/owner.lock`, held by an open descriptor, plus atomic metadata with PID, native start identity, version, and endpoint. Support local macOS volumes only and fail closed when identity cannot be verified.
- Worker decision: require a random 256-bit bearer token at `run/control.token` (`0600`), constant-time verification before routing, full log redaction, explicit `service rotate-token`, and native desktop/CLI access; never expose it to browser JavaScript or URLs.
- Worker decision: service states are `not_installed`, `stopped`, `starting`, `ready`, `degraded`, `stopping`, `failed`, and `incompatible`; install/start/stop/uninstall are idempotent and status is read-only.
- Worker decision: upgrade stages a version, quiesces/stops old while retaining runnable files, switches the active pointer, starts new, and commits only after lock, authenticated version health, DB/migration, scheduler reconciliation, and process recovery are ready; failure restores old. Authoritative owners never overlap.
- Worker decision: before any forward migration, create and verify a quiesced full-state snapshot and run a migration compatibility preflight; rollback stops the candidate, restores that snapshot atomically, validates it with the retained old binary, then restarts old. If a migration cannot be restored losslessly, the upgrade is rejected before pointer switch.
- Worker decision: backup quiesces mutations, uses SQLite online backup, copies config journal/sealed logs, writes checksums, excludes live PID ownership as restorable truth, and verifies checksum/schema/foreign keys before restore.
- Worker decision: plist label is `com.my-supervisor.daemon` with `RunAtLoad=true`, bounded `ThrottleInterval`, stable log paths, minimal environment, and idempotent `bootstrap`/`bootout` recovery in `gui/<uid>`.
- Worker decision: authentication covers every HTTP and WebSocket route, including health/version, in installed, standalone, and debug hosts. Each isolated debug root generates its own token; there is no unauthenticated exception. Read-only owner metadata supplies endpoint/version discovery before the authenticated handshake.
- Worker decision: CLI directly owns offline `install`, `start`, `stop`, `status`, `uninstall`, and recovery through the macOS service adapter; an online owner exclusively performs quiesce, backup, and upgrade preflight via authenticated RPC, while CLI coordinates launchd stop/start and lock handoff from the returned journal state.
- Worker decision: backup maintenance leaves supervised children running but blocks new process/job mutations, schedule claims, and log segment rotation; it drains in-flight transactions, seals logs, snapshots one logical cut, and either exits the barrier cleanly or fails without a partial backup.
- Worker decision: legacy source claims live under canonical `claims/<sha256(canonical_source_path)>.lock`, created only after atomically claiming the canonical root; source components are then opened no-follow and ownership/mode checked, avoiding writes inside an unclaimed legacy root.
- Worker decision: upgrade journal phases are `staged`, `quiesced`, `snapshot_verified`, `old_stopped`, `pointer_switched`, `candidate_ready`, `committed`, or `rolling_back`; journal and active pointer use temp-write, `fsync`, atomic rename, and parent-directory `fsync`. Restart/reboot resumes or rolls back from the last durable phase.
- Worker decision: debug interactive service evidence may override label/plist path only through `MSV_DAEMON_TEST_*` in debug builds; production label is fixed. Cleanup validates the exact override before removal.
- Worker decision: token rotation writes a new `0600` temp file, verifies owner/mode, fsyncs and renames atomically, increments credential generation, rejects new requests with the old generation immediately, allows already-authenticated in-flight requests to finish, closes old sessions/WebSockets, and makes clients rediscover/rebootstrap.
- Worker decision: every path component is opened no-follow and checked for expected user ownership/mode; state, version, helper, binary, token, plist, manifest, and pointer replacements are atomic with parent-directory fsync to close TOCTOU windows.
- Worker decision: this child implements service-only uninstall. Destructive data removal is not implemented; a future explicit command would require separate scope/approval.
- Every evidence row in `docs/evidence/macos-prod-09-service-owner.md` repeats revision, exact preparation/command, isolated root/label, precondition, action, expected/observed result, status, cleanup, and owning follow-up.

## Related Files / Entry Points
- `crates/daemon/src/lib.rs` — start with path resolution, dependency assembly, ownership, and recovery ordering.
- `crates/daemon/src/main.rs` — coordinate bootstrap, loops, shutdown, and service status.
- `crates/desktop/src/main.rs` — select installed-daemon client mode versus explicit embedded development mode.
- `crates/cli/src/main.rs` — expose supervisor service lifecycle, backup, upgrade, and recovery commands.
- `crates/platform/macos/src/launchd.rs` — reuse user-domain registration mechanics while separating supervisor and managed-process labels.
- `crates/desktop/ui/src/services/operations-client.ts` — preserve one operator contract in client and embedded modes.
- `crates/shared/src/api.rs` — define version/auth/session/service/backup/upgrade DTOs and stable errors.
- `crates/infra/http/src/lib.rs` — apply authentication before every HTTP/WebSocket route and register online maintenance RPC.
- `crates/infra/http/src/ws.rs` — authenticate upgrade/reconnect and close sessions on token rotation.
- `crates/cli/src/client.rs` — discover the token natively and preserve authenticated wire/error behavior.
- `docs/ARCHITECTURE.md` — document one-owner topology and recovery order.
- `docs/evidence/macos-prod-09-service-owner.md` (proposed) — record paths/permissions, authentication, lock, service state, backup, upgrade, rollback, and cleanup evidence.

## Execution Plan
### Stage 1 — Acquire one owner before durable work
- Starts when: The complete backend process, schedule, log, telemetry, and alert contracts compile, including the observability contract at `crates/application/src/events.rs`, and their daemon composition inputs are available.
- Work: Read-only discover canonical/legacy roots and reject unsafe symlinks; lock every existing source/target root in deterministic path order, or atomically create an absent canonical `0700` root as the ownership claim and immediately lock it before any other mutation; under all required locks establish paths/permissions, stage legacy migration, create/rotate credentials, and only then run migration, scheduler, recovery, or journal work.
- No-op when: A second daemon or desktop host already receives one deterministic client-or-error outcome before assembling authoritative loops.
- No-op handoff: Publish confirmed ownership at `crates/daemon/src/lib.rs` and record two-host/path/auth proof at `docs/evidence/macos-prod-09-service-owner.md`; continue to Stage 2/3 for unproven client/service behavior, or hand the completed child to parent/release when all stages are proven.
- Deliverable: Single-owner runtime assembly in `crates/daemon/src/lib.rs`.
- Verify: `bounded two-host ownership inspection`; Inputs: daemon and desktop hosts targeting one isolated data directory, including abrupt first-owner termination; Expected: one authority, no duplicate recovery loop, deterministic stale-owner recovery, and no pre-lock mutation.
- Ends when:
  - [ ] Ownership acquisition precedes all persistent and supervision side effects.
  - [ ] Lock contention reports owner identity and safe operator action.
  - [ ] Crash release/reacquisition cannot create overlapping active owners.
- Handoff: Stage 2 receives exclusive runtime ownership and owner discovery.
- Replan when: Filesystem locking semantics are insufficient on supported local volumes; combine atomic owner metadata with process liveness validation and fail closed on unverifiable ownership.

### Stage 2 — Make desktop and daemon roles explicit
- Starts when: Stage 1 provides exclusive ownership and discovery metadata.
- No-op when: Authenticated/versioned discovery and explicit embedded mode already produce every stable state without competing ownership or credential exposure; record proof and continue to Stage 3.
- Work: Make installed daemon authoritative through an authenticated/versioned loopback handshake, connect native desktop/CLI without exposing the credential to browser code, and restrict embedded runtime to an explicit development mode that also acquires ownership.
- Deliverable: One-owner desktop/daemon topology rooted in `crates/daemon/src/lib.rs`.
- Verify: `bounded authenticated host-mode inspection`; Inputs: `cargo check --workspace` and parent debug commands against one root for every HTTP/WebSocket/health route, available/unavailable/stale/incompatible, invalid/rotated token with in-flight request and reconnect cursor, and explicit embedded mode; Expected: compile `0`, no unauthenticated operation/pre-lock mutation/competing owner, user-only redacted credentials, deterministic session closure/rebootstrap, and stable state/exit results; record at `docs/evidence/macos-prod-09-service-owner.md`.
- Ends when:
  - [ ] Desktop quit cannot terminate an installed owner.
  - [ ] Client and embedded modes expose equivalent operations/error semantics.
  - [ ] Version incompatibility is explicit before mutating state.
- Handoff: Stage 3 receives stable authority and client discovery.
- Replan when: Current transport cannot negotiate compatibility safely; fail closed and introduce a version handshake before allowing the client to operate.

### Stage 3 — Install and upgrade the user service safely
- Starts when: Stage 2 separates installed authority from desktop client behavior.
- No-op when: Install/login/reboot/crash, service states, backup/snapshot, migration preflight, upgrade/rollback, service-only uninstall, and retained-data reinstall already pass with no orphan or credential leak; record proof and hand off to release gates.
- Work: Implement user LaunchAgent lifecycle, stable paths, helper resolution, backup, staged upgrade, readiness handoff, rollback, and uninstall/data separation.
- Deliverable: Recoverable supervisor service lifecycle at `crates/daemon/src/lib.rs`.
- Verify: `bounded interactive macOS launchd inspection`; Inputs: parent harness plus external legacy claims, component symlink/TOCTOU attempts, debug label override, install/login/reboot/crash, offline/online command authority, backup barrier success/timeout, token rotation, and process crash/reboot at every upgrade-journal phase; Expected: one owner, no pre-claim/source mutation, no unauthorized route/orphan/leak/partial backup, children remain running during backup, durable phase resumes/rolls back, old binary opens restored state, and data remains; record exact launchctl/CLI commands, journal/fsync observations, and cleanup at `docs/evidence/macos-prod-09-service-owner.md`.
- Ends when:
  - [ ] Lifecycle commands are idempotent and user-scoped.
  - [ ] Missing home/data/config/log/binary paths fail explicitly.
  - [ ] Upgrade keeps a recoverable prior owner until replacement readiness is proven.
- Handoff: `docs/briefs/2026-07-25-feat-macos-prod-08-operator-ui.md` receives installed single-owner behavior, credential/session lifecycle, discovery metadata, and the native proxy contract at `crates/daemon/src/lib.rs`; release gates consume this behavior only after the operator child proves transport parity.
- Replan when: launchd cannot safely replace the running binary in place or migration cannot prove lossless old-binary-readable snapshot restore; use staged version directories and an atomic active pointer, or reject the upgrade before mutation and return schema compatibility to the owning contract brief.

## Side Effect Checkpoints
- [ ] CLI, desktop, and standalone daemon never start competing runtime owners.
- [ ] Loopback bind and restrictive local-origin behavior remain unchanged.
- [ ] Supervisor LaunchAgent labels cannot collide with managed-process labels.
- [ ] User service installation never touches `/Library/LaunchDaemons` or requires root.
- [ ] Upgrade preserves foreign keys, config journals, outbox/cleanup state, handles, occurrences, queues, and log segments.
- [ ] Uninstall distinguishes service removal from data deletion and never deletes data implicitly.
- [ ] Owner replacement preserves caller cancellation, generation checks, process-group shutdown, deletion/config sagas, terminal outbox delivery, and dependency-signature idempotency.

## Acceptance Criteria
- [ ] Login, reboot, desktop launch, and daemon crash converge to exactly one owner without duplicate children or occurrences.
- [ ] Desktop connects to the installed owner and explicit embedded mode cannot acquire an already-owned data directory.
- [ ] Install, repeated install, crash recovery, upgrade rollback, and uninstall leave no orphan user LaunchAgent or helper.
- [ ] Missing or unsafe production paths fail with actionable errors and never fall back to `/tmp`.
- [ ] Backup/rollback and retained-data reinstall preserve authoritative state and stable identities.

## Open Questions
- None — production topology uses one user-scoped daemon owner and desktop acts as a client.
