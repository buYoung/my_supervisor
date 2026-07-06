# Brief Set: Wire core + daemon + Tauri + CLI + UI into a running operations console

## Purpose
- Turn the design-stage repo (empty `crates/`, mock-only UI) into a running operations console: a shared Rust app-core serves one operations Router that a headless daemon, a Tauri desktop host, and an `msv` CLI all expose or consume, with the existing UI rendering live data and macOS launchd (SystemRegistered) process management.
- Establish the devBridge — a **test-only** HTTP mirror of the Tauri `invoke` surface, separate from the daemon — plus the parity invariant (the devBridge endpoints and the invoke handlers call the same facade) so vibe-coding test automation drives the same functionality over HTTP instead of `invoke`.

## Child Briefs
- [x] `docs/briefs/2026-06-09-feat-host-wiring-01-foundation.md` — Operations app-core + axum Router contract + host assembly; exists because both hosts and the CLI must build against one shared core and one frozen Router contract.
- [x] `docs/briefs/2026-06-09-feat-host-wiring-02-daemon.md` — Headless daemon host; exists because the GUI-less my-supervisor is an independent host with its own lifecycle and signal concerns.
- [x] `docs/briefs/2026-06-09-feat-host-wiring-03-tauri-bridge.md` — Tauri desktop host (UI via `invoke`) + test-only in-process devBridge + tray; exists because the GUI host, its native invoke surface, and the test-only HTTP mirror form a distinct execution context. (Code complete + compiles; full GUI run needs a desktop session via `cargo tauri dev`.)
- [x] `docs/briefs/2026-06-09-feat-host-wiring-04-ui-wiring.md` — Wire Processes/Jobs/Logs UI to the live HTTP/WS API; exists because replacing mock data with the real client layer is independent frontend work.
- [x] `docs/briefs/2026-06-09-feat-host-wiring-05-cli.md` — `msv` CLI over the operations HTTP API; exists because the CLI is an independent thin-client binary consuming the same contract.
- [x] `docs/briefs/2026-06-09-feat-host-wiring-06-macos-service.md` — macOS launchd (SystemRegistered) process mode + convert flow; exists because OS-service registration is a platform-specific vertical (core `ManagementMode` + convert use case + `platform/macos` launchd adapter + convert route + convert UI) that layers onto the Direct-mode foundation.

## Execution Order
- Wave 1: `docs/briefs/2026-06-09-feat-host-wiring-01-foundation.md` runs alone — it pins the Router contract and assembly everything else builds on.
- Wave 2 (parallel after Wave 1): `docs/briefs/2026-06-09-feat-host-wiring-02-daemon.md`, `docs/briefs/2026-06-09-feat-host-wiring-03-tauri-bridge.md`, and `docs/briefs/2026-06-09-feat-host-wiring-05-cli.md` — each independently consumes the frozen contract.
- Wave 3: `docs/briefs/2026-06-09-feat-host-wiring-04-ui-wiring.md` after Wave 1 (needs the contract), developed against a running Wave-2 host; its build is embedded by the Tauri host.
- Wave 4: `docs/briefs/2026-06-09-feat-host-wiring-06-macos-service.md` after Wave 1, and best after the Direct-mode vertical proves out — it adds the convert route, the launchd adapter, and the convert UI.

## Dependencies
- `docs/briefs/2026-06-09-feat-host-wiring-02-daemon.md` depends on `docs/briefs/2026-06-09-feat-host-wiring-01-foundation.md` because it hosts the assembly's Router.
- `docs/briefs/2026-06-09-feat-host-wiring-03-tauri-bridge.md` depends on `docs/briefs/2026-06-09-feat-host-wiring-01-foundation.md` because its devBridge mounts the same Router and its invoke handlers call the same facade.
- `docs/briefs/2026-06-09-feat-host-wiring-05-cli.md` depends on `docs/briefs/2026-06-09-feat-host-wiring-01-foundation.md` because the CLI consumes the same `shared` DTOs and HTTP contract.
- `docs/briefs/2026-06-09-feat-host-wiring-04-ui-wiring.md` depends on `docs/briefs/2026-06-09-feat-host-wiring-01-foundation.md` for the contract and integrates with `docs/briefs/2026-06-09-feat-host-wiring-03-tauri-bridge.md` for the embedded WebView build and its injected token.
- `docs/briefs/2026-06-09-feat-host-wiring-06-macos-service.md` depends on `docs/briefs/2026-06-09-feat-host-wiring-01-foundation.md` for the `ManagementMode` domain and the convert-route slot, and on `docs/briefs/2026-06-09-feat-host-wiring-04-ui-wiring.md` where its convert UI extends the same Processes view.

## Parallelization
- `docs/briefs/2026-06-09-feat-host-wiring-02-daemon.md`, `docs/briefs/2026-06-09-feat-host-wiring-03-tauri-bridge.md`, and `docs/briefs/2026-06-09-feat-host-wiring-05-cli.md` can run in parallel — separate crates/binaries, all read-only consumers of child 01's assembly.
- `docs/briefs/2026-06-09-feat-host-wiring-04-ui-wiring.md` can start against the contract once a Wave-2 host serves, but coordinates with `docs/briefs/2026-06-09-feat-host-wiring-03-tauri-bridge.md` on the embedded build output and the injected token.
- `docs/briefs/2026-06-09-feat-host-wiring-06-macos-service.md` must not run in parallel with `docs/briefs/2026-06-09-feat-host-wiring-04-ui-wiring.md` where both edit `ProcessesView.tsx` (convert flow vs operations data) — serialize the Processes-view edits.
- `docs/briefs/2026-06-09-feat-host-wiring-02-daemon.md`, `docs/briefs/2026-06-09-feat-host-wiring-03-tauri-bridge.md`, and `docs/briefs/2026-06-09-feat-host-wiring-05-cli.md` must not all edit the root `Cargo.toml` `[workspace] members` at once — serialize that single edit.

## Conflict Hotspots
- `Cargo.toml` (root `[workspace] members`) — created by child 01; children 02, 03, 05, 06 each add a crate entry. Serialize edits.
- Child 01's composition/assembly + Router contract — the coupling surface for 02/03/04/05/06; freeze it at the end of Wave 1.
- `crates/desktop/ui/src/shared/types.ts` ↔ `crates/shared` wire DTOs — the parity surface (children 01, 04, 06).
- `crates/desktop/ui/src/features/processes/ProcessesView.tsx` — operations-data wiring (child 04) and the SystemRegistered convert flow (child 06) both edit it. Serialize.
- `crates/desktop/ui/dist` build output — produced by child 04, embedded by child 03.

## Shared Constraints
- Parity invariant: across all hosts, `tauri invoke` handlers and HTTP/WS routes are thin adapters over the **same** `application` facade — neither re-implements domain logic.
- Hexagonal invariants set-wide: `application` depends only on `core` ports (DD-017); `#[cfg(target_os)]` only in host DI under `app/*` (DD-018). The `AutoStartService` port is cross-platform; macOS launchd is implemented first, Linux systemd / Windows Service deferred (structure preserved).
- Networking: loopback-only (`127.0.0.1`), no authentication — both the daemon and the test-only devBridge follow DD-011's loopback-no-auth posture. The **devBridge is a test-only mirror** of the Tauri `invoke` surface, separate from the daemon and not the production transport; it writes its base URL to `~/Library/Application Support/my-supervisor/devbridge.json` (`{base_url}`) so an out-of-process test harness can discover its (possibly non-default) port. (Discovery contract shared by children 03/05.)
- UI transport (decided): the production Tauri UI calls `tauri invoke`; the standalone/browser UI talks HTTP/WS to the daemon. The devBridge mirrors the facade-backed `invoke` operations commands over HTTP, for test automation only.
- Quality bar: robust for daily personal use to a no-rough-edges standard — every slice ships with concrete acceptance criteria, error/edge handling, and graceful degradation. Operations (Process, Job, Log, macOS SystemRegistered) must be robust, not absent; the "robust or absent" rule governs only the later macOS window/hotkey automation (out of this set).

## Global Acceptance Criteria
- [ ] Parity: the devBridge HTTP endpoints and the Tauri `invoke` handlers call the same `application` facade, so an action driven over devBridge HTTP produces the same result as via `invoke`. (The daemon serves the same operations API separately, to the CLI and browser.)
- [ ] Starting/stopping a process over HTTP affects a real OS process and is reflected in the UI, against either host.
- [ ] The Processes/Jobs/Logs tabs render live data, and the `msv` CLI performs the same operations from the terminal.
- [ ] An automated test drives an operations action over the devBridge HTTP API (port discovered from `devbridge.json`), without `tauri invoke`.
- [ ] The devBridge and the daemon are both loopback-only with no auth (DD-011); the devBridge mirrors the facade-backed `invoke` operations commands.
- [ ] On macOS, a Direct process converts to SystemRegistered (launchd) and back, with rollback on failure and no orphaned plists.
- [ ] `cargo build --workspace`, `cargo tauri build`, and the `msv` binary all build; the daemon runs headless and the desktop app runs with a tray.

## Open Questions
- Should the devBridge be compiled out of release builds (test/dev-only), or always present (loopback-only)? (light default: present, loopback-only)
- devBridge default port — reuse the daemon's `9876` (only one host typically runs at once) or a distinct default so both can run together? (recommend reuse `9876`, override when co-running)
