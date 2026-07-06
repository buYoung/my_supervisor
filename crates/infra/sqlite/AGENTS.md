# AGENTS.md

## 1. Overview

`my-supervisor-infra-sqlite` implements process registry and job/run persistence over SQLite. A single `SqliteStore` backs both repository ports injected into the application layer.

## 2. Ownership Map

### Stable Ownership Boundaries

- **Schema boundary**: Start in `SqliteStore::migrate` when changing durable process, job, or run storage. It owns table names, columns, indexes, and WAL-backed connection setup; preserve compatibility with existing rows or add explicit migration logic.
- **Process repository boundary**: Start in the `StateRepository for SqliteStore` implementation when changing stored `ProcessSpec` or restart counter behavior. It owns ordering, upsert semantics, and Direct/SystemRegistered persistence fields consumed by `OperationsFacade`.
- **Job repository boundary**: Start in the `JobRepository for SqliteStore` implementation when changing job definitions or run history. It owns job/run upserts, run ordering, and run lookup by `(job_name, run_id)`.
- **Persistence representation boundary**: Start in `src/repr.rs` when changing JSON text representations for trigger and trigger-origin fields. These are local storage formats, not API DTOs; preserve round trips to `core` domain enums.

### Active Change Routes

- **SystemRegistered persistence route**: Within **Process repository boundary**, start in `spec_from_row` and `save_spec` when changing service registration fields. Keep `mode`, `unit_name`, and lifecycle string values aligned with domain mapping.

## 3. Core Behaviors & Patterns

- **One store, two ports**: `SqliteStore` implements both `StateRepository` and `JobRepository`; runtime assembly injects the same store into both `AppDeps` slots.
- **Local serialization**: args, env, triggers, and `TriggeredBy` are stored as JSON text columns using local `repr` types. API DTO serialization is not reused here.
- **Self-contained schema creation**: `connect()` and `connect_in_memory()` always call `migrate()` before returning a store.
- **Defensive decoding**: missing or malformed optional values generally fall back to domain defaults, while timestamp and trigger parse failures become `RepoError::Backend`.
- **Stable ordering**: list queries order processes and jobs by name; run history is ordered by scheduled time descending.

## 4. Conventions

- **Adapter errors**: helper `backend()` converts SQLx, parse, and serialization failures into `RepoError::Backend`.
- **String enums**: persisted simple enum values use lower snake_case strings such as `system_registered`, `detached`, and `run_anyway`.
- **Upserts**: save methods use `ON CONFLICT` to update mutable fields while preserving identities or counters where intended.
- **Mapping locality**: row-to-domain helpers live beside the repository implementation; complex JSON representations stay in `repr.rs`.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-infra-sqlite` after changes in this package.
