# AGENTS.md

## 1. Overview

This monorepo implements a local process and job supervisor with shared domain logic, host runtimes, platform adapters, a CLI, and a Tauri desktop UI.

## 2. Folder Structure

- `Cargo.toml`: Cargo workspace root; declares all Rust crates and shared dependency versions.
- `package.json`: root release and CI script manifest for repository-level automation.
- `crates/core`: domain entities and port traits; keep it free of workspace crate dependencies and adapter concerns.
- `crates/application`: transport-agnostic use cases centered on `OperationsFacade`; depends on `core` ports, not concrete adapters.
- `crates/shared`: serialized REST, WebSocket, error, and config DTOs shared by CLI, HTTP, desktop, config, and UI boundary code.
- `crates/config`: TOML config source adapter; maps `shared::config` DTOs into `core` domain models.
- `crates/infra`: infrastructure adapters behind `core` ports.
    - `http`: axum REST/WebSocket adapter and domain/wire mapping.
    - `sqlite`: `StateRepository` and `JobRepository` over SQLite.
    - `logging`: in-memory log sink with bounded tails and live broadcasts.
    - `scheduler`: Tokio scheduler for cron, interval, and one-shot job triggers.
- `crates/platform`: OS-specific adapter crates.
    - `macos`: Direct process lifecycle, Unix shutdown, and launchd registration.
- `crates/daemon`: shared runtime composition plus the thin `msv-daemon` launcher.
- `crates/cli`: `msv` command-line client over the operations API.
- `crates/desktop`: Tauri desktop host; embeds the shared runtime and contains the React/Vite UI under `ui`.
- `docs`: architecture, API, design decisions, roadmap, and task briefs; align behavior changes with these documents when relevant.

## 3. Working Agreements

- Respond in user's preferred language; if unspecified, infer from codebase (keep tech terms in English, never translate code blocks)
- Ask the user before introducing tests, lint, or formatter setups; add them only on explicit request
- Build context by reviewing related usages, flows, patterns, and likely impact before editing
- Fix the underlying cause, not only the visible symptom; inspect affected flows and apply the narrowest complete change that resolves the root issue
- Check side effects across callers, shared abstractions, and behavior/API boundaries; report relevant impact and compatibility risks
- Ask actively when user decisions are needed for scope, behavior, or tradeoffs
- Run type-check after Rust code changes with `cargo check --workspace`; use package-level `AGENTS.md` files for package-only verification guidance
- In monorepos, put package-only tests/type-check/verification guidance in the package-level AGENTS.md, not the root document
- New functions: single-purpose, colocated with related code
- External dependencies: only when necessary, explain why

## 4. Custom Instructions

- For any work on `fable5.md`, read that document first and treat its existing content as the source of truth.
