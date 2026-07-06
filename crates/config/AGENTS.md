# AGENTS.md

## 1. Overview

`my-supervisor-config` implements the TOML `ConfigSource` adapter. It parses file DTOs from `shared::config` and maps them into `core` domain config for application reload and bootstrap flows.

## 2. Ownership Map

### Stable Ownership Boundaries

- **Config load boundary**: Start in `src/lib.rs` when changing file reading, missing-file behavior, or `ConfigSource` implementation. It owns TOML parsing and empty-config fallback; preserve `ConfigError` behavior consumed by `OperationsFacade::reload`.
- **Config mapping boundary**: Start in `src/convert.rs` when changing how `ProcessConfigDto` or `JobConfigDto` becomes domain state. It must stay aligned with HTTP request mapping because both compile against the same shared DTOs.

### Active Change Routes

- **Process mode config route**: Within **Config mapping boundary**, start in `management_mode`, `lifecycle_mode`, and `process_spec` when config fields for Direct/SystemRegistered or tied/detached lifecycle change. Keep defaults consistent with `core` domain defaults.

## 3. Core Behaviors & Patterns

- **Adapter-only parsing**: TOML parsing stops at shared config DTOs, then converts into `LoadedConfig`; application code never sees raw TOML structures.
- **Missing file is valid**: `load()` returns `LoadedConfig::default()` for `NotFound`, allowing first-run bootstrap without a config file.
- **Mirrored DTO conversion**: this crate and `infra/http::mapping` both implement DTO to domain conversion to avoid a dependency cycle while sharing the same DTO definitions.
- **Domain defaults at boundary**: missing optional config fields become domain defaults for management mode, lifecycle, restart, shutdown, overlap policy, dependency failure policy, and log retention.

## 4. Conventions

- **Small conversion helpers**: keep enum/default mapping in focused functions such as `management_mode`, `lifecycle_mode`, and `job_trigger`.
- **No persistence or host logic**: this crate only loads and converts config; repository saves and scheduler registration stay in `application`.
- **Path handling**: convert wire string paths into `PathBuf` at this boundary and pass domain paths outward.
- **Error shape**: TOML parse failures become `ConfigError::Invalid`; filesystem failures other than missing files become `ConfigError::Io`.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-config` after changes in this package.
