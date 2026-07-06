# AGENTS.md

## 1. Overview

`my-supervisor-app-daemon` owns shared backend runtime assembly and the thin `msv-daemon` launcher. The desktop host reuses the same runtime composition in-process.

## 2. Ownership Map

### Stable Ownership Boundaries

- **Runtime assembly boundary**: Start in `src/lib.rs` when changing adapter wiring, data/config/log paths, default bind constants, or `AppDeps` construction. It owns the concrete adapter graph used by daemon and desktop hosts; preserve dependency injection through `core` ports.
- **Platform selection boundary**: Start in `platform_adapters` and `process_service_registrar` when changing OS-specific support. It owns `#[cfg(target_os)]` adapter selection and the non-macOS fallback registrar.
- **Launcher lifecycle boundary**: Start in `src/main.rs` when changing `msv-daemon` startup, bootstrap, scheduler loop spawning, HTTP serving, signal handling, or child reaping.

### Active Change Routes

- **Embedded host route**: Within **Runtime assembly boundary**, start in `build_runtime` and `build_deps` when changing behavior shared with desktop. Desktop calls this crate directly, so avoid assumptions that only the headless binary uses the runtime.

## 3. Core Behaviors & Patterns

- **Composition root**: this crate creates concrete config, SQLite, scheduler, logging, lifecycle, shutdown, registrar, and clock adapters, then calls `infra_http::assemble`.
- **Shared defaults**: `DEFAULT_BIND_ADDR`, `DEFAULT_BIND_PORT`, and `DEFAULT_BASE_URL` are consumed by hosts and CLI defaults.
- **Runtime paths**: state database and logs live under `data_dir()/my-supervisor`; config defaults to the platform config directory with a data-dir fallback.
- **Bootstrap before serving**: the launcher loads config, arms scheduler jobs, autostarts processes, then serves the operations router.
- **Graceful shutdown**: signal or API shutdown resolves the notify handle, stops the HTTP server, and reaps tied Direct children.

## 4. Conventions

- **Keep domain logic out**: runtime assembly may choose adapters and paths, but process/job behavior belongs in `application`.
- **Trait-object wiring**: new adapters should be injected through existing or new `core` ports, not exposed directly to hosts.
- **Platform cfg locality**: keep target-specific adapter selection in this crate or the platform crate; do not leak platform conditionals into `application`.
- **Contextful errors**: use `anyhow::Context` at host boundaries where path, bind, or server failures need operational detail.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-app-daemon` after changes in this package.
