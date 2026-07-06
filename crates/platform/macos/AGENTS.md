# AGENTS.md

## 1. Overview

`my-supervisor-platform-macos` implements macOS process-control adapters for Direct mode, graceful Unix shutdown, and SystemRegistered launchd management. Platform-specific and unsafe behavior is contained inside this crate.

## 2. Folder Structure

- `src/lib.rs`: module declarations and public adapter exports.
- `src/lifecycle.rs`: `MacLifecycle`, Direct-mode child tracking, aliveness probing, shutdown reaping, and transient job execution.
- `src/spawn.rs`: command construction, `setsid` process-group setup, stdout/stderr pumps, and child spawn plumbing.
- `src/shutdown.rs`: `UnixShutdown` graceful signal, grace-period polling, and force kill.
- `src/signals.rs`: thin libc signal helpers and signal constants.
- `src/launchd.rs`: `LaunchdAgentProcess`, LaunchAgent plist generation, launchctl lifecycle calls, status query, and log tailing.

## 3. Core Behaviors & Patterns

- **Process-group control**: Direct children are spawned in their own session via `setsid`, making `pid == pgid`; shutdown helpers signal the whole tree using negative pid values.
- **Log pumping at spawn boundary**: `spawn_child()` detaches stdout/stderr, and `attach_pumps()` routes lines either to a process source or a job run target.
- **Tracked Direct lifecycle**: `MacLifecycle` stores a UUID-to-alive flag map, waits on child exit in a background task, appends a system log line, and removes the child from tracking.
- **Graceful escalation**: `UnixShutdown` sends the configured signal, polls until the grace period expires, then sends `SIGKILL` to the process group if still alive.
- **LaunchAgent registration**: `LaunchdAgentProcess` writes per-process plist files under the user `~/Library/LaunchAgents`, bootstraps the GUI domain, and never writes the system domain.
- **Registration rollback**: failed launchctl bootstrap removes the newly written plist; application-level conversion also rolls back registrations if persistence fails.

## 4. Conventions

- **Unsafe containment**: libc calls and `pre_exec` usage stay in `signals.rs` and `spawn.rs`, with brief safety comments explaining the boundary.
- **Adapter exports**: expose concrete adapters from `lib.rs` only (`MacLifecycle`, `UnixShutdown`, `LaunchdAgentProcess`); keep helper modules private.
- **XML escaping**: plist string values must pass through the local `xml()` helper.
- **User domain only**: launchd targets use `gui/<uid>` and paths under the current user's home/log directories.
- **Best-effort platform cleanup**: `bootout`, log-dir creation, and plist removal may be best-effort where failures should not block a higher-level rollback.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-platform-macos` after changes in this package.
