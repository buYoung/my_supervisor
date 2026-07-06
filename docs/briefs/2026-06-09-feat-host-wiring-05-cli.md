# [feat] `msv` CLI over the operations HTTP API

## Work Type
feat

## Current State (As-Is)
- `crates/cli` does not exist; the `msv` binary described in `docs/ARCHITECTURE.md` §4.1.2 and `docs/DEVELOPMENT.md` §3 is unimplemented.
- The operations surface is HTTP/WS only (`docs/API.md`); there is no terminal client today.
- Per `docs/ARCHITECTURE.md` §4.6, `infra/http` and `app/cli` import the same wire DTOs, so the CLI speaks the host's contract by construction.
- The daemon binds `127.0.0.1:9876` (no auth, DD-011) and is the CLI's primary target; the Tauri devBridge (child 03) is a separate test-only feature, not a CLI concern.

## Desired Outcome (To-Be)
- `crates/cli` is a binary (`msv`) that drives the operations HTTP API over a configurable base URL: process and job listing/lifecycle, and log tailing, from the terminal.
- It is a thin client — it does NOT embed core; it reuses the `shared` wire DTOs (no forked types) and surfaces the `docs/API.md` §5 error envelope as human-readable errors and exit codes.
- Default target is the local daemon (`127.0.0.1:9876`, no token); `--url` overrides for other operations hosts. The test-only devBridge is normally driven by the test harness, not the CLI.
- Output is human-readable by default, with an `-o json` mode for scripting (per §4.1.2).

## Scope
### In Scope
- `crates/cli` binary crate + workspace member entry; CLI argument parsing.
- The flat command surface documented in `docs/ARCHITECTURE.md` §4.1.2: `msv start/stop/restart <name>`, `msv ps`, `msv logs <name> [-f]`, `msv add -c <config>`, `msv remove <name>`, `msv reload`, `msv daemon status`.
- An HTTP/WS client reusing `crates/shared` DTOs; a `--url` base-URL override; the `-o json` output mode (per §4.1.2).
- Surfacing the `docs/API.md` §5 error envelope as terminal-friendly messages, mapped onto the documented exit-code convention (`0` success, `1` general failure, `2` no such process, `3` daemon not running) per §4.1.2.

### Out of Scope
- [hard] Do not re-implement use cases or embed core — the CLI is a thin HTTP/WS client (unlike the hosts, which embed core).
- [hard] Do not add CLI-only operations that bypass the documented HTTP API.
- [deferred] Rule/automation commands (`msv rule …`) — Phase 2.
- [deferred] `msv daemon start|stop` (daemon self-launch) and `msv ui` (open the WebUI) — documented in §4.1.2 but deferred; this set wires operations, not daemon self-management. `msv daemon status` stays in.

## Constraints
- Reuse `crates/shared` DTOs; do not fork wire types or hand-roll JSON shapes.
- Default to the daemon's loopback `127.0.0.1:9876` (no auth, per DD-011) — the CLI is a daemon client. `--url` overrides the base URL for other operations hosts; the test-only devBridge (loopback no-auth) is normally driven by the test harness, not the CLI.
- Surface the `docs/API.md` §5 error envelope, not raw HTTP status text.

## Related Files / Entry Points
- `crates/cli/` (proposed) — new CLI binary crate location.
- `docs/ARCHITECTURE.md` — §4.1.2 (the `msv` CLI bin) and §4.6 (the shared-DTO note: the HTTP infra crate and the CLI import the same DTOs).
- `docs/API.md` — §2 (operations endpoints), §3 (WS), §5 (error envelope) the CLI targets.
- `docs/DEVELOPMENT.md` — §3 crate placement and the `my-supervisor-app-cli` package name.
- `docs/briefs/2026-06-09-feat-host-wiring-01-foundation.md` — provides the `shared` DTOs and the frozen Router contract.

## Side Effect Checkpoints
- [ ] The CLI imports `crates/shared` DTOs directly — no duplicated type definitions.
- [ ] Running `msv` against the daemon and against any other operations host via `--url` yields identical results.
- [ ] No CLI command reaches functionality absent from the documented HTTP API.

## Acceptance Criteria
- [ ] `msv` process-list shows live processes from the daemon; start/stop/restart affect a real OS process.
- [ ] `msv job ls`/run and `msv logs <name>` work against the running host.
- [ ] `-o json` emits machine-readable output; exit codes follow §4.1.2 (`1` general failure, `2` no such process, `3` daemon not running).
- [ ] `msv --url <host>` drives any operations host's HTTP API identically (the CLI is primarily a daemon client; the test-only devBridge is not its main target).

## Open Questions
- `docs/ARCHITECTURE.md` §4.1.2 also lists `msv daemon start|stop` (daemon self-launch) and `msv ui` (open WebUI) — include them now or defer? (recommend defer; this set wires operations, not daemon self-management)
- Adopt the documented output crates (`comfy-table`, `indicatif`) from §4.1.2 for tables/progress, or keep output minimal this slice? (recommend adopt the documented crates)
