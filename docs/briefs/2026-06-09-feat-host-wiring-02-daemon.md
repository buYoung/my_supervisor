# [feat] Headless daemon host serving the operations Router

## Work Type
feat

## Current State (As-Is)
- `apps/my_supervisor/crates/daemon` does not exist; the headless host described in `docs/ARCHITECTURE.md` §4.1.1 and `docs/DEVELOPMENT.md` §3 is unimplemented.
- The current mock UI header hardcodes a daemon at `127.0.0.1:9876` in `apps/my_supervisor/desktop/src/App.tsx`, which is the conventional local bind documented in `docs/DEVELOPMENT.md` §5.
- No process supervises the assembled Router yet; child 01 produces the composition/assembly this host will consume.

## Desired Outcome (To-Be)
- `apps/my_supervisor/crates/daemon` is a headless binary (`msv-daemon`) that calls child 01's composition function, binds the resulting axum Router to `127.0.0.1:9876` (HTTP + WS), and runs until signaled.
- The daemon owns process lifecycle: graceful startup, signal handling (SIGINT/SIGTERM), and graceful shutdown that stops supervised children and flushes the scheduler.
- It is the simplest host — no GUI, no tray, just Router + lifecycle. It does not embed a WebView and does not spawn or depend on `app/desktop`.
- Platform adapter selection (the `#[cfg(target_os)]` DI for OS-specific bits) lives in this host's DI per DD-018, injecting `None` for macOS-only automation seams that are not part of this slice.

## Scope
### In Scope
- `apps/my_supervisor/crates/daemon` binary crate plus its workspace member entry in the root `Cargo.toml`.
- Compose via child 01's assembly; bind the Router to `127.0.0.1:9876`; serve HTTP + WS.
- Signal handling (SIGINT/SIGTERM) and graceful shutdown of supervised Direct-mode processes and the scheduler.
- Host-level DI and config load via the `config` crate; `tracing` subscriber init honoring `RUST_LOG` per `docs/DEVELOPMENT.md` §5.

### Out of Scope
- [hard] Do not re-implement use cases or the Router — consume child 01's assembly only.
- [hard] No GUI, tray, or WebView (that is child 03).
- [deferred] systemd/launchd self-registration of the daemon process itself; multi-instance; remote (non-loopback) bind.
- [deferred] `app/cli` (`msv`) — separate follow-up; the daemon is reachable over HTTP regardless.

## Constraints
- Bind loopback only (`127.0.0.1`), matching the UI's expected `127.0.0.1:9876`.
- `#[cfg(target_os)]` is allowed here (host DI) but must not leak back into `core`/`application` (DD-018).
- Graceful shutdown must not orphan supervised child processes.

## Related Files / Entry Points
- `crates/.gitkeep` — workspace root anchor; the new daemon host crate is created here (see the proposed path below).
- `apps/my_supervisor/crates/daemon/` (proposed) — new headless host crate location.
- `docs/ARCHITECTURE.md` — §4.1.1 defines the headless daemon host (core embed, no daemon spawn); §4.1.3 is the separate Tauri desktop host.
- `docs/DEVELOPMENT.md` — §3 (crate placement, `msv-daemon` bin) and §5 (bind port, `RUST_LOG` filters).
- `apps/my_supervisor/desktop/src/App.tsx` — the `127.0.0.1:9876` endpoint the UI expects to reach.
- `docs/briefs/2026-06-09-feat-host-wiring-01-foundation.md` — prerequisite; provides the assembly and Router this host serves.

## Side Effect Checkpoints
- [ ] Sending SIGTERM stops the daemon and all supervised Direct-mode children, leaving no orphans.
- [ ] The Router served by the daemon is the same contract as child 03's devBridge (same child-01 assembly).
- [ ] `RUST_LOG=my_supervisor_app_daemon=debug` produces structured logs without panicking.

## Acceptance Criteria
- [ ] `cargo run -p my-supervisor-app-daemon` serves HTTP on `127.0.0.1:9876` and answers the daemon status/health route with 200.
- [ ] A process started via the daemon's HTTP API is actually running as an OS child and appears in `GET /api/v1/processes`.
- [ ] SIGINT/SIGTERM triggers graceful shutdown within a bounded time and leaves no orphaned children.

## Open Questions
- Confirm `127.0.0.1:9876` as the fixed default bind (overridable via config), matching the UI header. (recommend yes)
- Should a bounded graceful-shutdown timeout (after which remaining children are force-killed) be configurable, or is a fixed default acceptable for the skeleton? (recommend fixed default now)
