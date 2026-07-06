# AGENTS.md

## 1. Overview

`my-supervisor-shared` owns serialized DTOs for REST, WebSocket, errors, and TOML config. It is the contract crate shared by HTTP, CLI, desktop, config loading, and UI boundary code.

## 2. Ownership Map

### Stable Ownership Boundaries

- **API DTO boundary**: Start in `src/api.rs` when changing process, job, daemon, log, or conversion wire shape. These DTOs are consumed by HTTP handlers, CLI client, Tauri invoke mappings, config conversion, and frontend wire types; preserve `docs/API.md` compatibility.
- **Config schema boundary**: Start in `src/config.rs` when changing TOML file shape. Config loading maps these DTOs into `core` domain models; keep defaults and optional fields aligned with `config::convert`.
- **Error envelope boundary**: Start in `src/error.rs` when changing external error body format. HTTP and UI/CLI clients depend on the `{ error: { code, message } }` contract.

### Active Change Routes

- **SystemRegistered DTO route**: Within **API DTO boundary**, start with `ManagementModeDto`, `ConvertTargetDto`, and `ConvertRequestDto` when changing service-registration flows. Keep Rust DTOs, frontend `wire-types.ts`, and process mapping synchronized.

## 3. Core Behaviors & Patterns

- **Serde-owned wire shape**: DTO enums use `serde` attributes such as `rename_all = "snake_case"` and tagged enum representations to define the external JSON contract.
- **Wire/domain separation**: this crate does not depend on `core`; adapters translate shared DTOs to domain types in their own mapping modules.
- **Optional compatibility fields**: request DTOs use `#[serde(default)]` and `skip_serializing_if` for fields that may be omitted by config files or clients.
- **Single contract source**: CLI, HTTP, desktop invoke, and frontend wire declarations track these Rust DTOs rather than inventing parallel server-side shapes.

## 4. Conventions

- **DTO suffix**: serialized types use the `Dto` suffix; list wrappers use `*ListDto`; request bodies keep resource-specific names such as `ProcessConfigDto`.
- **JSON naming**: serialized fields are snake_case; frontend camelCase conversion happens only in UI mapping code.
- **Tagged enums**: multi-shape wire enums use `#[serde(tag = "type", rename_all = "snake_case")]`.
- **No domain logic**: do not add validation, scheduling, process lifecycle, or persistence decisions to DTO definitions.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-shared` after changes in this package.
