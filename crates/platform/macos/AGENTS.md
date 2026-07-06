# AGENTS.md

## 1. Overview

`my-supervisor-platform-macos` implements macOS process lifecycle, shutdown, and launchd registration adapters. It is the platform-specific boundary behind `core` ports for Direct and SystemRegistered process control.

## 2. Ownership Map

### Stable Ownership Boundaries

- **Direct lifecycle boundary**: Start in `src/lifecycle.rs` when changing spawn, probe, reap, or transient job execution. It owns `LifecycleController` behavior and must keep process/run log pumping wired through `LogSink`.
- **Spawn plumbing boundary**: Start in `src/spawn.rs` when changing command construction, session/process-group behavior, stdout/stderr capture, or log routing. Preserve `setsid` group ownership so shutdown can signal the full tree.
- **Shutdown boundary**: Start in `src/shutdown.rs` and `src/signals.rs` when changing graceful or forced stop behavior. It owns signal selection, grace-period polling, and group kill semantics for Direct-mode children.
- **Launchd registration boundary**: Start in `src/launchd.rs` when changing SystemRegistered behavior. It owns user-domain LaunchAgent plist generation, bootstrap/bootout/kickstart calls, status query, and launchd log tailing.

### Active Change Routes

- **Convert support route**: Across **Launchd registration boundary** and the application conversion flow, start in `LaunchdAgentProcess::register` when changing SystemRegistered setup. Keep conflict detection, plist cleanup on bootstrap failure, and user-domain targeting intact.
- **Job run capture route**: Within **Direct lifecycle boundary**, start in `run_transient` and `attach_pumps` when changing job execution output. Preserve routing to `LogTarget::Run(run_id)` instead of process-name logs.

## 3. Core Behaviors & Patterns

- **Own process group**: `spawn_child` creates a session with `setsid`, making `pid` usable as the process-group target for graceful and forced shutdown.
- **Line pump fan-out**: stdout/stderr are read asynchronously and appended to either process or run log channels based on `LogTarget`.
- **Tracked liveness**: `MacLifecycle` keeps an in-memory child map with an atomic alive flag and also probes the OS process.
- **Launchd user scope**: `LaunchdAgentProcess` writes only under `~/Library/LaunchAgents` and targets `gui/<uid>`, never the system domain.
- **Registration replacement**: register writes a plist, boots out a prior target best-effort, then bootstraps the fresh plist; failure removes the generated plist.

## 4. Conventions

- **Unsafe containment**: keep `libc` and `pre_exec` usage inside this platform crate with comments explaining the safety boundary.
- **Signal helpers**: raw signal numbers and `kill`/probe wrappers belong in `signals.rs`; higher-level shutdown and lifecycle code should call helpers.
- **XML escaping**: plist string values must pass through the local `xml()` helper.
- **Adapter exports**: expose concrete adapter types from `lib.rs`; do not export internal spawn or signal helpers.
- **Error translation**: map OS command and filesystem failures into the relevant `core::ports` error type at the adapter boundary.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-platform-macos` after changes in this package.
