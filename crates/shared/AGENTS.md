# AGENTS.md

## 1. Overview

`my-supervisor-shared` defines the serialized contracts shared by REST, WebSocket, CLI, config, desktop, and UI code. It is the single Rust source for API and config wire shapes.

## 2. Folder Structure

- `src/lib.rs`: exports shared API, config, error, and event modules.
- `src/api.rs`: REST DTOs for processes, jobs, logs, daemon status, restart no-op, and conversion.
- `src/config.rs`: TOML-facing config DTOs reused by config loading and CLI registration.
- `src/events.rs`: WebSocket event envelope DTOs.
- `src/error.rs`: uniform error envelope returned by HTTP and interpreted by clients.

## 3. Core Behaviors & Patterns

- **Wire contracts only**: DTOs represent external JSON/TOML shape, not application behavior. Domain defaults and lifecycle rules stay in `core` and `application`.
- **Snake_case wire format**: tagged enums and field names serialize to the documented API shape; frontend camelCase reconciliation happens in `crates/desktop/ui/src/services/wire-mapping.ts`.
- **Shared error envelope**: `ErrorBody` carries stable machine-readable codes and messages used by HTTP, CLI, and UI error handling.
- **Boundary reuse**: CLI, config, HTTP, desktop commands, and UI wire types depend on these DTOs so contract drift is caught by compilation or explicit mapping updates.

## 4. Conventions

- **DTO suffix**: serialized API types use `Dto` suffix (`ProcessStatusDto`, `JobConfigDto`, `RestartNoopDto`).
- **Serde tagging**: sum types that cross the wire use `#[serde(tag = "type", rename_all = "snake_case")]`; simple string enums use `#[serde(rename_all = "snake_case")]`.
- **Optional fields**: optional outbound fields use `skip_serializing_if = "Option::is_none"` where omission is part of the contract.
- **Collections**: environment maps use `BTreeMap<String, String>` for deterministic serialization.
- **No adapter imports**: keep this crate independent of `core`, `application`, `infra`, and host crates.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-shared` after changes in this package.
