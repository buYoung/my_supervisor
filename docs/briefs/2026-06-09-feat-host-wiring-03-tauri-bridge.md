# [feat] Tauri desktop host with invoke surface + test-only devBridge + tray

## Work Type
feat

## Current State (As-Is)
- No Tauri scaffolding exists: a repo scan finds no `src-tauri`, no `tauri.conf.json`, and no `.rs` files. The `crates/app/desktop` host from `docs/ARCHITECTURE.md` §4.1.3 is unimplemented.
- `packages/ui` is a Vite + React 19 app currently runnable standalone (`pnpm -C packages/ui dev`) on mock data; it is not yet loaded by any native shell.
- The docs define `app/desktop` as the GUI my-supervisor that embeds core in-process and must not spawn or depend on a daemon (DD-002 amended; `docs/ARCHITECTURE.md` §1 and §4.1.3).
- Decided (2026-06-09): the production Tauri UI ↔ core path is `tauri invoke`; the devBridge is a **test-only** HTTP mirror of that invoke surface, separate from the daemon.

## Desired Outcome (To-Be)
- `crates/app/desktop` is a Tauri v2 binary that calls child 01's composition function in-process and hosts a WebView loading `packages/ui`.
- The production UI ↔ core path is `tauri invoke`: thin invoke handlers cover the operations surface (start/stop/list, jobs, logs — delegating to the shared facade) plus native actions (tray, window control, notifications/permission prompts — native APIs). No domain logic in either.
- The **devBridge** is a **test-only** HTTP mirror: it mounts child 01's operations Router on loopback so test automation can drive the same operations over HTTP instead of `invoke`. It is separate from the daemon (the desktop host runs no daemon) and is not the production transport. Parity holds because each facade-backed `invoke` operations command and its matching devBridge HTTP endpoint call the same facade method.
- The devBridge binds `127.0.0.1` on a configurable port (default `9876`; override one when both run at once), loopback-only with **no authentication** — a test-only feature consistent with DD-011's loopback-no-auth posture. It writes its base URL to `~/Library/Application Support/my-supervisor/devbridge.json` (`{base_url}`, created on start, removed on exit) so an out-of-process test harness can discover the (possibly non-default) port.
- A tray icon keeps the app resident (close-to-tray, not quit), giving termination-resistance without a separate system daemon.

## Scope
### In Scope
- `crates/app/desktop` Tauri v2 crate plus `tauri.conf.json`, with `frontendDist` / dev URL pointed at `packages/ui`.
- In-process composition via child 01.
- Thin `tauri invoke` handlers: operations commands delegating to the shared facade (the production UI transport) + native actions via native APIs — no domain logic in either.
- The devBridge: mount child 01's operations Router on a configurable loopback port (no auth — test-only); write its base URL to the discovery file `~/Library/Application Support/my-supervisor/devbridge.json` for out-of-process test-automation discovery.
- A tray icon with close-to-tray behavior and an explicit quit affordance.

### Out of Scope
- [hard] Do not re-implement use cases or the Router — consume child 01's assembly.
- [hard] Do not spawn or require `app/daemon` (DD-002): the desktop host is self-contained.
- [hard] Do not put domain logic in invoke handlers or devBridge routes — both are thin adapters over the facade.
- [deferred] Window-management / global-hotkey / system-event automation (Rules, macOS) — Phase 3.
- [deferred] Code-signing, notarization, and installer packaging.
- [deferred] Authoring the automation test suite itself — this slice only makes the devBridge *drivable* by HTTP test automation; writing the tests is separate work (and not auto-generated, per repo convention).

## Constraints
- The devBridge binds `127.0.0.1` only, no authentication (test-only, loopback — consistent with DD-011). It is separate from the daemon and is not the production transport (the production UI uses `invoke`). Whether it is compiled out of release builds is a light open question.
- Parity: each facade-backed `invoke` operations command has a matching devBridge HTTP endpoint, and both call the identical facade method (native-only invoke commands — tray, window — have no HTTP mirror).
- Discovery contract (for test automation): the devBridge writes `~/Library/Application Support/my-supervisor/devbridge.json` containing `{ "base_url": "http://127.0.0.1:<port>" }` on start (removed on exit), so an out-of-process test harness finds the port. No auth token (loopback-only).
- Tauri v2 only, per `docs/ROADMAP.md` and `docs/ARCHITECTURE.md`.

## Related Files / Entry Points
- `crates/app/desktop/` (proposed) — new Tauri host crate location.
- `packages/ui/package.json` — frontend scripts and the build the WebView loads.
- `packages/ui/vite.config.ts` — dev server URL and build output dir the Tauri config must point at.
- `docs/ARCHITECTURE.md` — §1 and §4.1.3: host duality and "GUI my-supervisor embeds core, no daemon spawn".
- `docs/DESIGN_DECISIONS.md` — DD-002: host-dual rationale and the rejected "Tauri spawns daemon" option.
- `docs/DEVELOPMENT.md` — the "Tauri 앱" subsection in §4 (build/run commands) and §5 (WebView2, dev URL notes).
- `docs/briefs/2026-06-09-feat-host-wiring-01-foundation.md` — prerequisite; provides the assembly, Router, and facade.

## Side Effect Checkpoints
- [ ] Closing the window leaves the app resident in the tray (does not quit), and supervised processes keep running.
- [ ] Each devBridge HTTP endpoint and its matching `invoke` operations handler call the same facade method — driving either produces the same result (parity verified by code review).
- [ ] An automated test can drive every operations action over the devBridge HTTP API (discovering the port from `devbridge.json`) without using `tauri invoke`.
- [ ] The devBridge is loopback-only (`127.0.0.1`) and not reachable off-loopback; it has no auth (test-only), like the separate daemon (DD-011).

## Acceptance Criteria
- [ ] `cargo tauri dev` launches the desktop app loading `packages/ui` in a WebView, with the UI driving operations via `invoke`.
- [ ] The devBridge answers the operations routes (e.g. `GET /api/v1/processes`) on its loopback port — mirroring the facade-backed `invoke` commands — with no auth.
- [ ] Starting a process via the devBridge HTTP API starts a real OS child, verified via the API response (and equivalently via the matching `invoke` command).
- [ ] Every `tauri invoke` handler delegates to the shared facade or a native API — none contains domain logic (verified by code review).
- [ ] Closing the main window keeps a tray icon present; quitting from the tray terminates cleanly with no orphaned children.

## Open Questions
- Should the devBridge be compiled out of release builds (test/dev-only), or always present (loopback-only)? (light default: present, loopback-only)
- devBridge default port — reuse the daemon's `9876` (only one host typically runs at a time), or pick a distinct default so the daemon and desktop can run simultaneously? (recommend reuse `9876`, override when co-running)
- Exact crate/dir layout for the Tauri host — follow the docs' `crates/app/desktop` placement vs Tauri's conventional `src-tauri`? (recommend follow the docs and adapt the Tauri config)
