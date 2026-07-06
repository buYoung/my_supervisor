# [feat] Operations app-core + axum Router contract + host assembly

## Work Type
feat

## Current State (As-Is)
- `crates/` holds only `crates/.gitkeep`; there is no root `Cargo.toml`, so no Rust workspace exists yet.
- The Hexagonal layering, crate names (`my-supervisor-*` prefix), and workspace member globs are specified in `docs/DEVELOPMENT.md` §3 and `docs/ARCHITECTURE.md` §3, but no code realizes them.
- Frontend wire shapes already exist in `apps/my_supervisor/desktop/src/shared/types.ts` (`ProcessStatus`, `JobStatus`, `JobRun`, `LogLine`, `DaemonStatus`), but they do not literally match the documented wire: `docs/API.md` §4 is snake_case (`restart_count`, `started_at`, `cpu_percent`, `management_mode`, `triggered_by`) while `types.ts` is camelCase and carries derived/divergent fields: `ProcessStatus.uptime` and a nullable `unit_name`/`pid` (Direct mode → `management_mode: direct`, `unit_name: null`); `JobRun.triggeredBy` as a flat string vs the API's tagged `triggered_by` object; and log lines, where the wire is `{timestamp, stream, line}` only but `types.ts` `LogLine` adds `id`/`source`. The authoritative wire contract is `docs/API.md` §4 (snake_case JSON); the Rust `shared` DTO keeps the full §4 shape (including the nullable `unit_name` and `ManagementMode`) even though SystemRegistered *behavior* lands in child 06, and reconciling the FE camelCase is child 04's job, not a change to the wire.
- The HTTP/WS surface — routes, request/response bodies, error envelope — is fully specified in `docs/API.md` §2–§5 but unimplemented; WS log follow is per-process / per-run (`/api/v1/processes/{name}/logs`, `/api/v1/jobs/{name}/runs/{run_id}/logs`), not a single global tail.
- No composition root exists that both hosts (`app/daemon`, `app/desktop`) can call to build the wired use cases and obtain a ready Router.

## Desired Outcome (To-Be)
- A Cargo workspace exists with the foundation crates: `core` (domain entities + port traits), `application` (use cases over ports only), `shared` (HTTP/WS wire DTOs + config schema), `config` (TOML `ConfigSource`), and `infra/{http,sqlite,scheduler,logging}`.
- `infra/http` exposes a single `axum` `Router` (REST + WS) whose route list and request/response types are pinned to `docs/API.md` for the operations surface (Processes, Jobs, Logs, daemon status/health).
- A documented composition/assembly entry point builds the wired use cases with concrete adapters and returns the `Router` plus lifecycle handles, so `app/daemon` and `app/desktop` can each host the identical Router without re-implementing wiring.
- The operations use cases are callable through the Router at walking-skeleton depth: Process Direct-mode lifecycle (start/stop/restart/list/add/remove) against real OS processes; Job list/add/run/remove with cron/interval/one-shot trigger evaluation and run history; Log capture from managed processes with tail + WS stream; daemon status/health.
- The use-case layer is exposed as a transport-agnostic facade that any host adapter (HTTP route or Tauri invoke) can call without duplicating domain logic — the precondition for the parity invariant implemented in child 03.

## Scope
### In Scope
- `apps/my_supervisor/Cargo.toml` workspace with member globs per `docs/DEVELOPMENT.md` §3 (`crates/core`, `crates/application`, `crates/shared`, `crates/config`, `crates/infra/*`, `crates/platform/*`, `crates/daemon`, `crates/cli`, `desktop/src-tauri`).
- `core`: `Process`, `Job` (+ `JobRun` and trigger types), `Log` domain types and the port traits they need — `LifecycleController`/`ChildHandle`, `StateRepository`, `JobRepository`, `Scheduler`, `LogSink`, `ConfigSource`, `Clock`.
- `application`: use cases for the operations surface, depending only on `core` ports (DD-017).
- `shared`: wire DTOs matching `apps/my_supervisor/desktop/src/shared/types.ts` and `docs/API.md` §4, plus the error envelope from `docs/API.md` §5.
- `infra/http`: the axum Router (REST + WS) translating HTTP/WS ↔ use-case facade; a concrete, enumerated route+type contract. The Router enumerates the in-scope operations endpoints (Process Direct lifecycle, Jobs CRUD incl. `PATCH /api/v1/jobs/{name}` update + trigger + runs history, per-process/per-run Logs WS, daemon status/reload/shutdown, a `GET /api/v1/health` liveness probe, and the global events WS); the `convert` route (`POST /api/v1/processes/{name}/convert`) is omitted from foundation's Router and added by child 06, the `/api/v1/rules*` family stays deferred (Phase 2), and `POST /api/v1/jobs/{name}/runs/{run_id}/cancel` is deferred (it needs transient-run process tracking) — none is stubbed.
- `infra/sqlite`, `infra/scheduler`, `infra/logging`, `config`: concrete adapters at walking-skeleton depth (real enough to serve live data).
- A composition/assembly function returning `(Router, lifecycle handles)` for hosts to consume. The Router is returned **unbound** — each host owns its bind address/port (the daemon binds `127.0.0.1:9876`; the Tauri devBridge binds its own configurable loopback port).
- The `LifecycleController`/`ChildHandle` trait shape — the irreducible spike that must later accommodate a Windows Job Object; design the trait now, implement the unix/macOS Direct-spawn path.

### Out of Scope
- [hard] `app/daemon`, `app/desktop`, `app/cli` host/CLI binaries — those are children 02/03/05 and out of this brief.
- [hard] OS-service adapters (`platform/*`) and the SystemRegistered convert flow — foundation is Direct-mode-first; macOS launchd SystemRegistered is child 06. `ManagementMode` is modeled in the domain here (so `ProcessStatus` carries it), but only the `Direct` variant is operational and the `convert` route is added by child 06.
- [deferred] Rule/automation domain (`EventSource`, window/hotkey) — Phase 2/3.
- [deferred] Job `depends_on` dependency chains and overlap policy beyond a single default; log rotation/backpressure/run-archive; multi-process resource limits.
- [deferred] `POST /api/v1/jobs/{name}/runs/{run_id}/cancel` — needs transient-run process-group tracking so a running run can be signalled; Phase 2 (not stubbed, absent from the manifest).
- [deferred] Wire-type codegen — manual parity between the `shared` crate and `types.ts` this slice.

## Constraints
- `application` depends only on `core` ports, never on `platform/*` or concrete infra (DD-017); `#[cfg(target_os)]` must not appear in `core`/`application` (DD-018).
- The in-scope Router contract (every operations route + its request/response type) must be concrete and kept in sync with `docs/API.md`, because children 02 and 03 build against it independently — an underspecified contract makes the Wave 2 parallelism fake. `docs/API.md` §2/§4/§5 is the contract of record; the enumerated `infra/http` route list is the frozen manifest children 02/03/04 diff against.
- The wire is snake_case JSON per `docs/API.md` §4: the Rust `shared` crate serializes snake_case (serde default / explicit rename). The FE camelCase reconciliation (casing + `uptime`/`triggeredBy` divergence) lives in child 04's client layer; this brief does not change `types.ts` or fork the wire shape.
- The Router serves an unauthenticated, loopback-only contract per DD-011 ("인증 없음", `127.0.0.1` only) — the assembly returns a bare Router and does not embed auth; binding and any optional host-specific hardening are the hosts' concern (children 02/03).
- The use-case facade must be transport-agnostic: no `axum`/HTTP types may leak into facade signatures, so a Tauri invoke adapter can call the identical facade.

## Related Files / Entry Points
- `crates/.gitkeep` — workspace root anchor; create the foundation crate directories alongside it.
- `crates/core/` (proposed) — first crate to create; domain + port traits.
- `docs/DEVELOPMENT.md` — §3 is the authoritative crate tree, workspace member globs, and `my-supervisor-` package-name prefix.
- `docs/ARCHITECTURE.md` — §3 (layer responsibilities), §4.2 (use-case list), §4.3 (domain + ports tables).
- `docs/API.md` — §2 (operations endpoints), §4 (wire types), §5 (error envelope): the Router contract source of truth.
- `apps/my_supervisor/desktop/src/shared/types.ts` — the TS wire shapes the Rust `shared` crate must mirror.
- `docs/DESIGN_DECISIONS.md` — DD-017 ~ DD-024 are the layering invariants this work must satisfy.

## Side Effect Checkpoints
- [ ] The `application` crate compiles with no `platform/*` or concrete-infra dependency (DD-017 holds).
- [ ] No `#[cfg(target_os)]` appears in `core` or `application` (DD-018 holds).
- [ ] The composition function returns a Router a host can serve without re-wiring use cases.
- [ ] `shared` wire DTOs serialize the snake_case shapes of `docs/API.md` §4 for Process/Job/Log/Daemon (the FE camelCase reconciliation is child 04's responsibility, not this crate's).
- [ ] Router routes and bodies match the in-scope operations subset of `docs/API.md` §2/§4/§5 with no code↔doc drift (out-of-scope convert/rules routes intentionally absent).

## Acceptance Criteria
- [ ] `cargo build --workspace` succeeds with the foundation crates present.
- [ ] Driving the assembled Router over HTTP starts a real OS process (Direct mode), and `GET /api/v1/processes` reflects its live state and PID.
- [ ] `GET /api/v1/jobs` returns jobs whose cron/interval/one-shot next-run is computed by the scheduler; running a job records a `JobRun`.
- [ ] A managed process's stdout/stderr is retrievable via the logs endpoint and streamed over the WS route.
- [ ] The in-scope Router contract (every operations route with its request/response type) is enumerated and matches `docs/API.md`, with out-of-scope routes (convert, rules, cancel-run) intentionally omitted.
- [ ] The use-case facade has no HTTP/axum types in its public signatures.

## Open Questions
- `app/cli` (`msv`) and macOS SystemRegistered are confirmed in scope as sibling children (`05-cli`, `06-macos-service`); foundation models `ManagementMode` in the domain and leaves the `convert` route slot for child 06 — confirm no further launchd-specific shaping is needed in the facade now. (recommend: model `ManagementMode` here, add `convert` in child 06)
- Is manual `shared`↔`types.ts` wire-type parity acceptable for this slice, or should codegen be set up now? (recommend manual now)
