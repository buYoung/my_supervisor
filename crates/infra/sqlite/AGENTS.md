# AGENTS.md

## 1. Overview

`my-supervisor-infra-sqlite` persists process specs, restart counters, job definitions, and job run history in SQLite. One `SqliteStore` implements both repository ports.

## 2. Folder Structure

- `src/lib.rs`: SQLite connection setup, schema migration, row/domain conversion, and `StateRepository`/`JobRepository` implementations.
- `src/repr.rs`: JSON text representations for complex domain enum values stored in SQLite columns.

## 3. Core Behaviors & Patterns

- **Single store, two ports**: host composition injects the same `Arc<SqliteStore>` into `state_repo` and `job_repo`; keep process and job persistence cohesive unless port boundaries change.
- **Startup migration**: `connect()` and `connect_in_memory()` both call `migrate()` before returning, so callers never manage schema setup.
- **WAL persistence**: file-backed stores use SQLite WAL mode with a small connection pool; in-memory stores use one connection for ephemeral hosts or tests.
- **Loss-tolerant legacy parsing**: JSON vectors/maps default to empty on malformed data, unknown process modes fall back to Direct, unknown lifecycle falls back to Tied, and unknown run states fall back to Pending.
- **Stable ordering**: process and job list queries sort by `name`; run history sorts by `scheduled_at DESC` with a caller-provided limit.
- **Upsert semantics**: process and job saves use `ON CONFLICT` updates. Process saves preserve existing `restart_count`; run saves update final fields for an existing `run_id`.

## 4. Conventions

- **Backend error wrapper**: convert SQLx, parse, and serialization errors through the local `backend()` helper into `RepoError::Backend`.
- **String conversion helpers**: keep timestamp, state, and JSON conversion helpers near row mapping code.
- **Persistence-only reprs**: `TriggerRepr` and `TriggeredByRepr` are local storage formats, not API DTOs. Do not reuse them across HTTP or config boundaries.
- **SQL shape**: schema DDL stays in `migrate()` and each repository method owns its explicit query; avoid hiding table access behind broad generic helpers.
- **Domain defaults on read**: fields not currently persisted, such as restart and shutdown policy details, are restored with domain defaults.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-infra-sqlite` after changes in this package.
