# AGENTS.md

## 1. Overview

`my-supervisor-app-daemon` is the shared backend composition crate and the thin `msv-daemon` launcher. Desktop and headless hosts reuse this runtime wiring.

## 2. Folder Structure

- `src/lib.rs`: runtime constants, data/config path helpers, adapter construction, platform selection, and `build_runtime()`/`build_deps()`.
- `src/main.rs`: launcher entry point, tracing setup, bootstrap, scheduler loop, HTTP bind, graceful shutdown, and child reaping.

## 3. Core Behaviors & Patterns

- **Composition root**: this crate wires config, SQLite, HTTP, logging, scheduler, clock, lifecycle, shutdown, and registrar adapters into `AppDeps`.
- **Shared host runtime**: both `msv-daemon` and the Tauri desktop host call `build_runtime()`, keeping API behavior and facade wiring identical.
- **Path ownership**: data, logs, SQLite state, and config paths are resolved here before injection. Application code receives only `DaemonMeta` and port trait objects.
- **Platform cfg boundary**: macOS adapter selection happens in `platform_adapters()` and `process_service_registrar()`; non-macOS Direct runtime support is explicitly deferred.
- **Launcher lifecycle**: `main.rs` bootstraps config/scheduler/autostart, spawns the scheduler loop, serves axum on the default loopback bind address, waits for signal or API shutdown, then reaps tied children.

## 4. Conventions

- **Constants**: keep `DEFAULT_BIND_ADDR`, `DEFAULT_BIND_PORT`, and `DEFAULT_BASE_URL` in this crate for CLI and host reuse.
- **Thin binary**: `src/main.rs` should remain startup and shutdown orchestration only; use-case behavior belongs in `application`.
- **Adapter construction**: create concrete adapters in `build_deps()` and immediately erase them behind `Arc<dyn Trait>` where `AppDeps` requires it.
- **Error context**: use `anyhow::Context` around external startup failures such as opening SQLite, binding TCP, or serving HTTP.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-app-daemon` after changes in this package.
