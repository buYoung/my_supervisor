# AGENTS.md

## 1. Overview

`my-supervisor-config` implements the TOML `ConfigSource` adapter and maps file DTOs into core domain models. It is the file-config counterpart to the HTTP request mapping layer.

## 2. Folder Structure

- `src/lib.rs`: `TomlConfigSource`, file reading, TOML parsing, and `ConfigSource` implementation.
- `src/convert.rs`: explicit mapping from `shared` config/API DTOs to `core` `ProcessSpec` and `Job` values.

## 3. Core Behaviors & Patterns

- **Absent config is empty config**: `load()` treats `NotFound` as `LoadedConfig::default()` so first-run hosts can bootstrap without a file.
- **DTO-to-domain mapping**: config parsing first deserializes `shared::config::FileConfig`, then converts into `core` models with domain defaults for restart, shutdown, lifecycle, overlap, dependency, and retention fields.
- **Mirrored adapter mapping**: `convert.rs` intentionally mirrors HTTP DTO-to-domain conversion instead of sharing a module that would create a dependency cycle. Both mappings compile against `shared` DTOs.
- **Error boundary**: TOML syntax and validation failures become `ConfigError::Invalid`; file-system failures other than absence become `ConfigError::Io`.

## 4. Conventions

- **Small converter functions**: map enum fragments in focused helpers (`management_mode`, `lifecycle_mode`, `job_trigger`) before composing full entities.
- **Default handling**: use DTO optional fields to select domain defaults; do not encode alternate defaults in the loader.
- **No persistence or runtime effects**: this crate only loads and converts configuration. Repository writes and scheduler registration belong to `application::OperationsFacade::reload`.
- **Path ownership**: `TomlConfigSource` stores a `PathBuf` and returns clones from `path()`; callers own placement.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-config` after changes in this package.
