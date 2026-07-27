# AGENTS.md

## 1. Overview

This monorepo implements a local process and job supervisor with shared domain logic, host runtimes, platform adapters, a CLI, and a Tauri desktop UI.

## 2. Ownership Map

### Stable Ownership Boundaries

- **Domain and port boundary**: Start in `crates/core` when changing process, job, log, config, or external capability contracts. It owns domain entities and port traits consumed by every higher layer; verify through the downstream workspace type-check surface because `application`, `infra/*`, `platform/*`, `daemon`, `cli`, and `desktop` compile against these contracts.
- **Use-case boundary**: Start in `crates/application` when changing process/job behavior, daemon lifecycle, scheduling orchestration, config reload, or error semantics. It owns `OperationsFacade`, application views, domain events, and `AppError`; preserve transport independence and verify through HTTP, Tauri, and CLI consumers.
- **Wire contract boundary**: Start in `crates/shared` and `crates/infra/http` when changing REST, WebSocket, error, or config DTO shape. `shared` owns serialized types, while `infra/http` owns route registration and domain/wire mapping; preserve `docs/API.md` compatibility and update UI/CLI mapping consumers together.
- **Runtime composition boundary**: Start in `crates/daemon` when changing adapter wiring, default bind values, data paths, bootstrap, or host shutdown behavior. It owns the shared runtime used by both `msv-daemon` and `desktop`; preserve platform selection through `core` ports.
- **Host client boundary**: Start in `crates/desktop` for Tauri invoke/devBridge/UI behavior and in `crates/cli` for command-line output and exit-code behavior. Both consume the same operations API and `shared` DTOs; keep host behavior thin over the facade contract.

### Active Change Routes

- **Host wiring route**: Across **Runtime composition boundary** and **Host client boundary**, start in `crates/daemon/src/lib.rs` and `crates/desktop/src/main.rs` when changing embedded-runtime behavior. The current code intentionally shares one `OperationsFacade` between Tauri invoke and devBridge; verify both transports still map through `infra_http::mapping`.
- **SystemRegistered process route**: Within **Use-case boundary**, start in `OperationsFacade::convert_process` and the `ProcessServiceRegistrar` port when changing Direct/SystemRegistered conversion. Keep registration-before-persistence and rollback behavior aligned with `crates/platform/macos/src/launchd.rs`.
- **Documentation contract route**: Across **Wire contract boundary**, start in `docs/API.md`, `crates/shared/src/api.rs`, and `crates/infra/http/src/lib.rs` when changing endpoint or DTO semantics. Keep the route manifest, DTOs, CLI client, and desktop service mappings synchronized.

## 3. Working Agreements

- Respond in user's preferred language; if unspecified, infer from codebase (keep tech terms in English, never translate code blocks)
- Ask the user before introducing tests, lint, or formatter setups; add them only on explicit request
- Build context by reviewing related usages, flows, patterns, and likely impact before editing
- Fix the underlying cause, not only the visible symptom; inspect affected flows and apply the narrowest complete change that resolves the root issue
- Check side effects across callers, shared abstractions, and behavior/API boundaries; report relevant impact and compatibility risks
- Ask actively when user decisions are needed for scope, behavior, or tradeoffs
- Run type-check after Rust code changes with `cargo check --workspace`; use package-level `AGENTS.md` files for package-only verification guidance
- Put package-only tests/type-check/verification guidance in the package-level AGENTS.md, not the root document
- New functions: single-purpose, colocated with related code
- External dependencies: only when necessary, explain why

## 4. Custom Instructions

- Absolute rule for `codemap-search`: actively use `codemap-search` for code exploration and repository navigation. Prefer it over generic Read, Grep, Find, shell search, or broad file-reading workflows whenever it is available and suitable; do not skip this rule for convenience.
- `my_supervisor`는 pm2 상위호환 process manager 개념이고, gui, cli로 관리하도록 하는것이 주 목적
  - `my_supervisor`의 core는 실제 프로세스를 관리하는 신뢰해야하는 신뢰메인프로세스임.
