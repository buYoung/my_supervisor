# [feat] macOS launchd (SystemRegistered) process mode + convert flow

## Work Type
feat

## Current State (As-Is)
- The foundation (child 01) implements Direct-mode process lifecycle only; `ManagementMode` is modeled in the domain but only the `Direct` variant is operational, and `POST /api/v1/processes/{name}/convert` is not yet served.
- `docs/API.md` §2 defines `convert` (body `{ to, unit_name?, auto_start? }`) with error codes `409 unit_name_conflict` and `service_registration_failed`, and `restart` is a documented no-op for SystemRegistered (DD-025).
- `crates/desktop/ui/src/features/processes/ProcessesView.tsx` already renders a SystemRegistered registration *preview* (launchd plist + `launchctl` commands via `getServiceRegistrationPreview`) and a "관리 모드 전환" panel with a rollback message — but it only copies static snippets; nothing is registered.
- `docs/ARCHITECTURE.md` (§4.3 ports, §6.4, §11.2) defines `ProcessServiceRegistrar` as the per-process SystemRegistered port (distinct from the daemon-autostart `AutoStartService`), implemented by a `platform/macos` `LaunchdAgentProcess` adapter; `application::StartProcess` branches on `management_mode` between `LifecycleController` (Direct) and `ProcessServiceRegistrar` (SystemRegistered). `crates/platform/macos` does not exist yet.

## Desired Outcome (To-Be)
- A `ProcessServiceRegistrar` port (in `core`) — the per-process SystemRegistered registration port (register/unregister/start/stop/query_status/tail_logs keyed on `unit_name`), distinct from the daemon's own `AutoStartService` — with a macOS `LaunchdAgentProcess` adapter (`platform/macos`) that generates the per-process LaunchAgent plist, bootstraps/enables it via `launchctl bootstrap/bootout`, and aggregates status via `launchctl list`.
- `POST /api/v1/processes/{name}/convert` is served: Direct ↔ SystemRegistered transitions that persist `ManagementMode`, with `restart` becoming the documented no-op for SystemRegistered (DD-025).
- A failed conversion rolls back to the prior mode and leaves no orphaned plist (matching the rollback case the mock UI already depicts).
- The Processes view's SystemRegistered panel performs a real convert on macOS (not just snippet copy); on non-macOS hosts the convert-to-system path returns a clear `not supported on this platform`.
- macOS-first: launchd implemented now; Linux systemd / Windows Service are structurally allowed for via the same port but not implemented this set.

## Scope
### In Scope
- `core` `ProcessServiceRegistrar` port (per-process register/unregister/start/stop/query_status/tail_logs, keyed on `unit_name`) and the `ManagementMode` transition modeling; the convert use case in `application` (depends on the port only), branching `StartProcess` between `LifecycleController` (Direct) and `ProcessServiceRegistrar` (SystemRegistered) per §6.4.
- `crates/platform/macos` `LaunchdAgentProcess` adapter: per-process plist generation, `launchctl bootstrap/bootout/enable`, status via `launchctl list`; host DI wiring (`#[cfg(target_os = "macos")]`) selecting it.
- Serving `POST /api/v1/processes/{name}/convert` in `infra/http` with the §5 error codes (`unit_name_conflict`, `service_registration_failed`); `restart` no-op for SystemRegistered (DD-025).
- Wiring the real convert flow into `ProcessesView.tsx` — replace snippet-copy-only with an actual convert call, keeping the preview as a "what will be applied" confirmation.
- Rollback-on-failure, idempotent registration, and no orphaned plists.

### Out of Scope
- [hard] Linux systemd and Windows Service adapters — structure preserved via the port, not implemented this set.
- [hard] Do not call `launchctl` or write plists outside the `platform/macos` adapter (no ad-hoc OS calls in `application`/`infra/http`).
- [deferred] Auto-start-on-login policy UI beyond the convert `auto_start` flag.
- [deferred] Aggregating launchd logs into the Logs view for SystemRegistered processes — Direct-process logs only this set.

## Constraints
- The `ProcessServiceRegistrar` port is cross-platform; `#[cfg(target_os)]` selection lives only in host DI (DD-018), never in `core`/`application`.
- Convert must be transactional from the user's view: on adapter failure, persist nothing, restore the prior `ManagementMode`, and remove any partially-written plist.
- Writes target the user LaunchAgents domain (`~/Library/LaunchAgents`, `gui/$(id -u)`) — no system-domain (root) registration this set.

## Related Files / Entry Points
- `crates/platform/macos/` (proposed) — new macOS launchd adapter crate.
- `crates/desktop/ui/src/features/processes/ProcessesView.tsx` — the existing SystemRegistered preview (`getServiceRegistrationPreview` macOS branch) and the "관리 모드 전환" panel to make real.
- `docs/API.md` — §2 `convert` route and §5 error codes (`unit_name_conflict`, `service_registration_failed`); the SystemRegistered `restart` no-op.
- `docs/ARCHITECTURE.md` — §4.3/§11.2 `ProcessServiceRegistrar` (per-process port) vs `AutoStartService` (daemon autostart); §6.4 management-mode branch; `LaunchdAgentProcess` in the platform crate table.
- `docs/DESIGN_DECISIONS.md` — DD-025 (SystemRegistered restart delegation) and DD-018 (cfg only in host DI).
- `docs/briefs/2026-06-09-feat-host-wiring-01-foundation.md` — provides the `ManagementMode` domain, the facade, and the convert-route slot.

## Side Effect Checkpoints
- [ ] A failed convert leaves the process in its original mode with no plist remaining in `~/Library/LaunchAgents`.
- [ ] Re-running a convert that already happened is idempotent (no duplicate-registration error).
- [ ] On a non-macOS host, convert-to-SystemRegistered returns a clear `not supported on this platform` rather than a panic or silent no-op.
- [ ] `restart` on a SystemRegistered process returns the documented no-op response (DD-025), not a Direct-style restart.
- [ ] `core` and `application` contain no `#[cfg(target_os)]` after this slice (DD-018 holds).

## Acceptance Criteria
- [ ] On macOS, converting a Direct process to SystemRegistered writes a valid LaunchAgent plist, `launchctl` loads it, and the process is then managed by launchd.
- [ ] Converting back to Direct unregisters the LaunchAgent and resumes Direct supervision.
- [ ] A deliberately failed registration (e.g., a `unit_name` conflict) returns `409 unit_name_conflict` and rolls back with no residual plist.
- [ ] The Processes view performs a real convert on macOS (not just snippet copy), reflecting the resulting mode.
- [ ] Building on a non-macOS target still compiles (port present, adapter absent), and convert-to-system is rejected with the platform message.

## Open Questions
- Should the convert UI keep showing the copyable launchd snippet (as a "what will be applied" confirmation) alongside the real action, or replace it entirely? (recommend keep as confirmation)
- The default `unit_name` generation scheme when the user supplies none — confirm `com.my-supervisor.managed.<name>`, matching the mock preview? (recommend yes)
