# [feat] Complete production operator surfaces

## Work Type
feat

## Current State (As-Is)
- [confirmed] As of `a1c845c` on `main`, the process screen performs real create/start/stop/restart/convert/delete calls and shows current CPU/memory, but its panel still says the form is presentation-only — Evidence: `handleAddProcess()`, row actions, and `PanelHeader` in `crates/desktop/ui/src/features/processes/ProcessesView.tsx`.
- [confirmed] The process screen contains Linux and Windows service previews and hard-coded example user paths even though runtime assembly rejects non-macOS hosts — Evidence: `detectServicePlatform()` and `getServiceRegistrationPreview()` in `ProcessesView`, and `platform_adapters()` in `crates/daemon/src/lib.rs`.
- [confirmed] The job screen lists all trigger kinds but its creation form can create only cron jobs and exposes only overlap policy — Evidence: `handleAddJob()` and form controls in `crates/desktop/ui/src/features/jobs/JobsView.tsx`.
- [confirmed] The desktop `JobRunState` omits `timed_out` even though the shared Rust DTO, SQLite, CLI, runner, and events all support it — Evidence: `crates/desktop/ui/src/shared/types.ts` compared with `JobRunStateDto::TimedOut` in `crates/shared/src/api.rs`.
- [confirmed] Run history fetches up to 20 runs separately for every job on each jobs poll and silently converts per-job failures to empty arrays — Evidence: `Promise.all()` run merge in `JobsView`.
- [confirmed] Frontend features consume `OperationsClient`, and invoke/HTTP adapters are expected to remain equivalent — Evidence: `crates/desktop/AGENTS.md` and `crates/desktop/ui/src/services/operations-client.ts`.
- [inferred] Production-scale instance, run, metric, and alert history will make current fan-out polling and silent partial failures operationally misleading — Confirm by: populating many jobs and forcing one run-history request to fail.

## Desired Outcome (To-Be)
- Desktop, CLI, HTTP, and Tauri expose the complete production process and schedule contracts with equivalent behavior.
- Process workflows support create/edit, instance detail, scale, guardrail configuration, group actions, rolling restart progress, and actionable failures.
- Job workflows support every trigger and production policy, schedule preview, edit, pause/resume where supported, manual trigger, run cancellation, retry/occurrence detail, and bounded history filtering.
- Logs, metrics, operational events, and alerts are searchable, paginated, cursor-aware, and explicit about partial failure.
- The desktop is macOS-specific and contains no unsupported Linux/Windows service instructions or placeholder claims.
- Timed-out runs and every production state render safely and accessibly.

## Scope
### In Scope
- Extend shared/API operations and both frontend transports for all production capabilities.
- Replace basic process and job forms with complete validated create/edit workflows.
- Add instance, rollout, occurrence/attempt, metrics, event, alert, and bounded log views.
- Add pagination/cursors or aggregate endpoints to avoid unbounded per-job request fan-out.
- Correct `timed_out`, placeholder text, unsupported platform previews, disabled/no-op actions, and partial-error handling.
- Keep CLI output scriptable with stable JSON and meaningful exit codes.
### Out of Scope
- [hard] Do not expose Linux systemd or Windows Service controls in the macOS product.
- [hard] Do not expose a UI toggle before the backend capability and persistence contract are complete.
- [deferred] Code signing, notarization, packaging, and final gates belong to `docs/briefs/2026-07-25-build-macos-prod-10-release-gates.md`.
- [hard] Do not add new test files or test cases without separate user approval.

## Constraints
- Start from the observability and alert contract at `crates/application/src/events.rs`; all backend production briefs must be complete before final UI acceptance.
- Add methods to `OperationsClient` first, then implement invoke and HTTP clients with equivalent errors and mappings.
- Preserve snake_case on wire DTOs and camelCase in UI types.
- Keep destructive actions explicit and show partial per-instance/per-run results.
- Never hide a backend request failure by replacing it with an empty successful state.
- Worker decision: maintain a capability matrix in `docs/API.md` with rows for process definition/group/instance/rollout, job definition/preview/occurrence/attempt/cancel, logs, metrics, events, alerts/acknowledgement, daemon/service status, and config; each row names HTTP, invoke, CLI, DTO, error, and pagination behavior.
- Worker decision: bearer authentication is injected by the native HTTP/CLI client from the user-only credential file and is never returned to or stored in browser JavaScript, local storage, UI state, logs, URLs, or WebSocket query strings.
- Worker decision: production Tauri invoke and UI HTTP abstractions are thin native proxies to the installed daemon; they inject the bearer outside the webview and never assemble an authoritative facade. Direct facade invoke exists only in explicit embedded-development mode and still obeys the data-root lock.
- Worker decision: debug devBridge exchanges the bearer through a one-time native command for a `10m` daemon session cookie (`HttpOnly`, `SameSite=Strict`, exact loopback `Path`, no `Domain`) plus a non-secret CSRF nonce held only in memory. Mutation fetches carry the nonce; WebSocket upgrade uses the cookie plus strict Origin. Daemon restart, token rotation, logout, or `15m` absolute age revokes sessions and native bootstrap resumes the same log cursor after reauthentication.
- Worker decision: list endpoints default to `50` and cap at `200`; cursors are stable opaque backend cursors. One panel refresh issues one aggregate request per resource family with client concurrency at most `4`; partial failure preserves prior data and reports failed partitions.
- Worker decision: capability discovery first records existing CLI commands, JSON fields, and exit meanings. Preserve current `--output json` byte-level schema for existing commands; add opt-in `--output json-v2` with versioned `ok`, `data`, `error`, and `partial` envelope and exit codes `0` success, `1` domain failure, `2` usage/validation, `3` partial, `4` daemon/auth/transport unavailable.

## Related Files / Entry Points
- `crates/desktop/ui/src/services/operations-client.ts` — start with one transport-neutral capability contract.
- `crates/desktop/ui/src/services/invoke-client.ts` — implement production Tauri operations.
- `crates/desktop/ui/src/services/http-client.ts` — preserve HTTP/WebSocket parity.
- `crates/desktop/ui/src/services/wire-types.ts` — mirror additive backend DTOs accurately.
- `crates/desktop/ui/src/services/wire-mapping.ts` — map every state and policy without loss.
- `crates/desktop/ui/src/features/processes/ProcessesView.tsx` — complete process and instance workflows.
- `crates/desktop/ui/src/features/jobs/JobsView.tsx` — complete schedule, occurrence, retry, and cancellation workflows.
- `crates/desktop/ui/src/features/logs/LogsView.tsx` — integrate bounded segmented log cursors.
- `crates/desktop/ui/src/features/daemon/DaemonView.tsx` — present recovery and operational readiness.
- `crates/cli/src/main.rs` — expose production commands and stable output.
- `crates/cli/src/client.rs` — preserve authenticated HTTP DTO/error/pagination semantics for CLI commands.
- `crates/desktop/src/main.rs` — add Rust Tauri invoke commands and native credential injection without exposing secrets to webview state.
- `crates/shared/src/api.rs` — define shared production wire DTOs and error envelopes.
- `crates/infra/http/src/lib.rs` — register additive production routes.
- `crates/infra/http/src/handlers.rs` — map application operations and partial results.
- `crates/infra/http/src/mapping.rs` — keep HTTP, Tauri, and shared domain/wire meaning aligned.
- `crates/application/src/facade.rs` — add aggregate/filter query use cases without transport coupling.
- `crates/core/src/ports/repository.rs` — define stable cursor/page query operations.
- `crates/infra/sqlite/src/lib.rs` — implement indexed bounded queries and partition outcomes.
- `docs/API.md` — keep public routes and DTOs synchronized.
- `docs/evidence/macos-prod-08-operator-ui.md` (proposed) — record transport parity, UI workflow, scale, pagination, partial failure, and accessibility evidence.

## Execution Plan
### Stage 1 — Stabilize cross-transport operations
- Starts when: All predecessor backend contracts compile and `docs/briefs/2026-07-25-build-macos-prod-09-service-owner.md` has proven at `crates/daemon/src/lib.rs` one installed owner, credential/session lifecycle, authenticated discovery, and native proxy behavior.
- Work: Build the capability/legacy-output matrix; add application/repository aggregate queries; then extend `OperationsClient`, wire types, mappings, native daemon-proxy invoke commands, HTTP routes/session middleware, and CLI operations until every row has equivalent payload, error, auth, pagination, and reconnect semantics.
- No-op when: HTTP, invoke, CLI, and UI type contracts already expose every production operation and state with equivalent error semantics.
- No-op handoff: Parent and release brief receive the confirmed installed-owner operator contract at `crates/desktop/ui/src/services/operations-client.ts`; continue only after transport parity is demonstrated against the service-owner evidence.
- Deliverable: Complete transport-neutral operator contract in `crates/desktop/ui/src/services/operations-client.ts`.
- Verify: `bounded transport parity inspection`; Inputs: `cargo check --workspace`, `cargo test --workspace`, UI typecheck, baseline command-by-command stdout/stderr/exit captures for existing `--output json`, the matrix, and success/validation/not-found/conflict/partial/auth/rotation/restart/session-expiry/CSRF/Origin/WebSocket-cursor fixtures through native proxy, HTTP, CLI, and devBridge; Expected: commands exit `0`, rows and meanings match, existing JSON semantic fields/values and exits remain compatible, v2 is deterministic, sessions revoke/rebootstrap without cursor loss, and bearer never crosses browser state; record canonical fixtures/diffs at `docs/evidence/macos-prod-08-operator-ui.md`.
- Ends when:
  - [ ] Every backend capability has equivalent invoke and HTTP semantics.
  - [ ] CLI JSON exposes stable machine-readable aggregate and per-item outcomes.
  - [ ] `timed_out` and every new state are exhaustive in wire and UI types.
- Handoff: Stage 2 receives the completed matrix in `docs/API.md`, transport-neutral interface in `crates/desktop/ui/src/services/operations-client.ts`, and matching wire types/mappings; every matrix row is marked implemented with payload/error/auth/pagination semantics.
- Replan when: A transport cannot preserve an existing error code or payload; stop and version the additive wire contract before UI work.

### Stage 2 — Complete process and scheduling workflows
- Starts when: Stage 1 provides every operation through `OperationsClient`.
- No-op when: Both HTTP and invoke already complete every matrix workflow with matching submitted DTOs, validation, risky partial results, macOS-only copy, and preserved failure state; record proof and continue to Stage 3.
- Work: Build complete validated process/job edit flows, instance and rollout controls, trigger/policy configuration, preview, occurrence detail, and cancellation/retry actions.
- Deliverable: Production process and scheduling workflows in the desktop and CLI.
- Verify: `bounded operator workflow inspection`; Inputs: `pnpm --dir crates/desktop/ui typecheck`, parent debug daemon/CLI fixture setup, desktop devBridge HTTP and Tauri invoke launches, two instances, every trigger/policy/action, validation failure, and partial result; Expected: typecheck `0`, UI requests match visible values on both transports, failures preserve data, and unsupported copy is absent; record launch commands, matrix rows, screenshots/JSON, and cleanup at `docs/evidence/macos-prod-08-operator-ui.md`.
- Ends when:
  - [ ] Operators can configure every supported production field without editing raw SQLite data.
  - [ ] Risky operations show scope and per-item results.
  - [ ] Unsupported platform content and placeholder/no-op copy are removed.
- Handoff: Stage 3 receives `docs/API.md` rows marked workflow-complete and the implemented process/job views plus `OperationsClient` paths; unresolved rows or failed evidence remain blocking.
- Replan when: A backend field lacks safe user-facing validation or description; pause that control and correct the API/documented contract rather than guessing.

### Stage 3 — Add scalable diagnostics and verify parity
- Starts when: Process and schedule workflows operate through both frontend transports.
- No-op when: The declared scale, partial-failure, expired-cursor, accessibility, and two-transport cases already pass against the installed-owner contract within request/page/concurrency limits; record proof and hand the completed operator contract to release gates.
- Work: Add bounded logs, metrics, events, alerts, pagination/cursors, partial-failure reporting, and accessibility states; verify HTTP and invoke behavior.
- Deliverable: Production operator surface at `crates/desktop/ui/src/services/operations-client.ts`.
- Verify: `bounded operator scale inspection`; Inputs: `pnpm --dir crates/desktop/ui build`, parent debug harness populated through public API/CLI with `250` jobs, `50` instances, `10,000` entries, one debug-injected partition failure, expired cursor, keyboard-only flow, and both desktop transports; Expected: build `0`, request/page/concurrency bounds hold, prior data/partial error/cursor boundary remain visible, and no state/accessibility crash occurs; on failure fix the owning transport/view, rerun its stage evidence, then rerun Stage 1 parity and Stage 3; record at `docs/evidence/macos-prod-08-operator-ui.md`.
- Ends when:
  - [ ] Diagnostics remain responsive and bounded as process/job counts grow.
  - [ ] Invoke and HTTP paths show the same state, actions, and error meaning.
  - [ ] Alerts can be acknowledged and recovery state is visible.
- Handoff: `docs/briefs/2026-07-25-build-macos-prod-10-release-gates.md` and the parent receive the installed-owner operator contract at `crates/desktop/ui/src/services/operations-client.ts`.
- Replan when: Production history cannot be fetched without unbounded fan-out; add an aggregate paginated backend query before completing the view.

## Side Effect Checkpoints
- [ ] Existing process and job create/start/stop/restart/delete commands remain available.
- [ ] Tauri invoke and devBridge HTTP clients use the same facade and shared mapping semantics.
- [ ] Live-log reconnect preserves numeric cursor and high-watermark behavior across rotation.
- [ ] Partial request failures remain visible and do not erase previously loaded data.
- [ ] Close-to-tray supervision and explicit quit behavior remain distinguishable.
- [ ] Every icon-only action has an accessible label and disabled/busy state.

## Acceptance Criteria
- [ ] Operators can create and edit every supported process and job policy from the macOS desktop.
- [ ] Process groups expose per-instance health, readiness, resource, generation, and rollout progress.
- [ ] Job detail exposes timezone/DST, preview, occurrence, attempts, retry, queue, cancellation, and final outcome.
- [ ] `timed_out` and all production states render with a defined label and visual tone.
- [ ] HTTP, Tauri invoke, CLI JSON, and UI agree on operation outcome and error codes.
- [ ] `pnpm --dir crates/desktop/ui typecheck` and `pnpm --dir crates/desktop/ui build` exit `0`.

## Open Questions
- None — the production operator surface follows the complete backend contract and macOS-only scope.
