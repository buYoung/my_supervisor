# [feat] Wire Processes/Jobs/Logs UI to the live backend (invoke + HTTP)

## Work Type
feat

## Current State (As-Is)
- Every view imports static mock arrays directly: `packages/ui/src/features/processes/ProcessesView.tsx`, `packages/ui/src/features/jobs/JobsView.tsx`, and `packages/ui/src/features/logs/LogsView.tsx` all read from `packages/ui/src/shared/mock-data.ts`. There is no `services/` layer, transport client, or state library.
- `packages/ui/src/App.tsx` hardcodes the daemon-connected indicator (`127.0.0.1:9876`, "데몬 연결됨") and renders 5 tabs (`processes/jobs/logs/daemon/settings`) from `NavigationKey`.
- `packages/ui/src/shared/types.ts` defines camelCase shapes (`ProcessStatus`, `JobStatus`, `JobRun`, `LogLine`, `DaemonStatus`), but the backend wire (`docs/API.md` §4) is snake_case (`restart_count`, `started_at`, `triggered_by`, …) and `types.ts` carries derived/divergent fields (`ProcessStatus.uptime`; `JobRun.triggeredBy` flat vs the API's tagged `triggered_by`; the wire log line is `{timestamp, stream, line}` only, but `LogLine` adds `id`/`source`) — so the client must map between them.
- The app dependencies are minimal (React 19 + lucide-react only); there is no data-fetching or state-management library.
- Decided transport (2026-06-09): the production Tauri UI calls `tauri invoke`; the standalone/browser UI talks HTTP/WS to the daemon. The devBridge (child 03) is a test-only HTTP mirror, not the UI's transport.

## Desired Outcome (To-Be)
- A `services/` layer under `packages/ui/src` exposes a **transport-agnostic** operations client with two adapters behind one interface: `invoke` (used when running inside Tauri — the production path) and HTTP + WS (used standalone against the daemon). The three views consume the interface, not a specific transport.
- Both adapters map the snake_case wire shapes to the camelCase `types.ts` shapes and reconcile the divergent fields: `ProcessStatus.uptime` (derived from `started_at`), `JobRun.triggeredBy`, and the log lines — the wire log line is `{timestamp, stream, line}` only, so the client synthesizes `LogLine.id` (a stable list key) and fills `LogLine.source` from the selected process name. `types.ts` stays the FE source of truth; nothing is forked onto the wire.
- The Processes, Jobs, and Logs views render live backend data instead of mock arrays, with loading and error states; the Logs view follows a single selected process's logs (per-process: `/api/v1/processes/{name}/logs` over WS, or the matching `invoke` follow command).
- The header daemon-connection indicator reflects real reachability — `GET /api/v1/daemon/status` (or its `invoke` equivalent) returning OK — rather than a hardcoded string.

## Scope
### In Scope
- A `services/` layer with a transport interface and two adapters: an `invoke` adapter (Tauri) and an HTTP-fetch + WS adapter (standalone/daemon). No auth token — both the daemon and the test-only devBridge are loopback no-auth.
- A wire-mapping layer shared by both adapters: snake_case ↔ camelCase, plus the `uptime` / `triggeredBy` / `LogLine.id`+`source` reconciliation.
- Replace mock consumption in `ProcessesView`, `JobsView`, and `LogsView` with the live client, plus loading/error/empty states.
- Wire Process actions (start/stop/restart/add/remove) and Job actions (run/add/remove) through the transport interface.
- Wire the Logs view to a selected process's per-process log-follow (WS over HTTP, or the invoke follow command).
- Wire the header connection indicator to daemon status reachability.

### Out of Scope
- [hard] Do not change the visual design system or the `packages/ui/src/components/ui/primitives.tsx` API — wire data through existing components.
- [deferred] Full wiring of the Daemon tab and Settings tab — they remain mock/static this slice (only the header indicator is wired).
- [deferred] The Rules tab and the 동등 이원 IA re-organization (Phase 2).
- [deferred] Wire-type codegen; adoption of a client-side state-management library.

## Constraints
- The services layer is transport-agnostic: in Tauri the UI uses `invoke`; standalone it uses HTTP/WS — selected behind one interface, with no domain logic in either adapter (parity, mirroring child 03's invoke/HTTP split).
- No auth token is sent — the daemon and the test-only devBridge are both loopback no-auth (DD-011). The standalone HTTP adapter uses a configurable base URL.
- Keep `packages/ui/src/shared/types.ts` as the single source of FE types; both adapters map the snake_case wire onto these camelCase shapes — do not fork shapes or push camelCase onto the wire.

## Related Files / Entry Points
- `packages/ui/src/shared/mock-data.ts` — the current data source to replace with the live client.
- `packages/ui/src/shared/types.ts` — wire types to keep aligned with the backend `shared` crate.
- `packages/ui/src/features/processes/ProcessesView.tsx` — Processes view to wire.
- `packages/ui/src/features/jobs/JobsView.tsx` — Jobs view to wire.
- `packages/ui/src/features/logs/LogsView.tsx` — Logs view to wire to per-process log-follow.
- `packages/ui/src/App.tsx` — header indicator to wire; nav stays at 5 tabs this slice.
- `docs/API.md` — §2/§4/§5: the endpoints, types, and error envelope the HTTP adapter targets (the invoke adapter mirrors the same operations).
- `docs/briefs/2026-06-09-feat-host-wiring-01-foundation.md` — the Router contract + facade both transports consume.
- `docs/briefs/2026-06-09-feat-host-wiring-03-tauri-bridge.md` — defines the Tauri invoke handlers the invoke adapter calls.

## Side Effect Checkpoints
- [ ] The Daemon and Settings tabs still render (on mock data) without runtime errors after the services layer lands.
- [ ] The same view code works through either adapter (invoke in Tauri, HTTP standalone) — no transport-specific branching leaks into the views.
- [ ] snake_case wire fields decode into the camelCase `types.ts` shapes (e.g., `restart_count`→`restartCount`, `started_at`→`startedAt`) with no `undefined` from casing mismatch, on both adapters.
- [ ] Any `packages/ui/src/shared/types.ts` field change stays consistent with the snake_case wire of the Rust `shared` crate.

## Acceptance Criteria
- [ ] In the Tauri app, the Processes/Jobs/Logs tabs show live backend data via `invoke` (no mock arrays).
- [ ] Standalone against the daemon, the same three tabs show live data via HTTP/WS.
- [ ] Process start/stop/restart and Job run/add/remove from the UI take effect on the backend and reflect on refresh, through whichever adapter is active.
- [ ] The Logs view tails a selected process's new lines in near-real-time.
- [ ] snake_case wire values render correctly in the UI (e.g., a process's `restart_count` and `started_at` appear, not blank), confirming the mapping works end-to-end.
- [ ] Loading and error states render when the backend is unreachable, with no blank crash.

## Open Questions
- Should the three views poll on an interval or rely on a WS/event push for live updates beyond the Logs tail? (recommend interval polling for Processes/Jobs this slice, streaming only for Logs)
- For the standalone HTTP adapter, where should the daemon base URL config live — build-time env (`VITE_*`) or a runtime setting? (recommend `VITE_*` with a `127.0.0.1:9876` default)
