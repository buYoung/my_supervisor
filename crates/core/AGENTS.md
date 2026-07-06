# AGENTS.md

## 1. Overview

`my-supervisor-core` defines the supervisor domain model and all port traits used by higher layers. It keeps process control, persistence, scheduling, config, logging, and service registration independent from concrete hosts and adapters.

## 2. Ownership Map

### Stable Ownership Boundaries

- **Domain contract boundary**: Start in `src/domain` when changing `ProcessSpec`, `Job`, `JobRun`, log lines, or loaded config. These types are persisted, serialized through adapter mappings, and consumed by `OperationsFacade`; preserve enum variants and default semantics unless every mapper and store is updated.
- **Port trait boundary**: Start in `src/ports` when changing external capabilities such as lifecycle control, repositories, scheduling, shutdown, logging, config loading, or service registration. The traits own cross-crate contracts implemented by adapters; verify by compiling every implementing crate.
- **Process mode boundary**: Start in `src/domain/process.rs` and `src/ports/registrar.rs` when changing Direct versus SystemRegistered behavior. `ManagementMode` decides whether lifecycle flows through `LifecycleController`/`ShutdownSignaler` or `ProcessServiceRegistrar`; preserve the `unit_name` contract for platform adapters.

### Active Change Routes

- **Job/process split route**: Within **Domain contract boundary**, start in `src/domain/job.rs` when changing transient run-to-completion behavior. Keep `JobRunState::is_terminal`, `TriggeredBy`, and `JobTrigger` aligned with scheduler, repository, and DTO mappings.
- **Repository persistence route**: Within **Port trait boundary**, start in `src/ports/repository.rs` when changing process registry or job history persistence. Both `StateRepository` and `JobRepository` are implemented by one SQLite store, so signature changes affect runtime assembly and application orchestration together.

## 3. Core Behaviors & Patterns

- **Ports and adapters**: `core` owns traits; `application` depends on those traits; `infra/*`, `platform/*`, and `config` implement them. New external behavior should enter through a narrow port rather than importing adapter details into domain code.
- **Management mode split**: `ManagementMode::Direct` uses in-daemon spawn/probe/stop semantics; `SystemRegistered` uses an OS service manager through `ProcessServiceRegistrar`. Keep mode-specific data on the enum and propagate it through mappings.
- **Transient jobs**: `Job` and `JobRun` model run-to-completion work separately from supervised `ProcessSpec`. Job execution uses trigger and run-state types instead of process runtime state.
- **Boundary errors**: port errors describe backend failures without HTTP, Tauri, CLI, or UI concepts. Outer layers convert these errors to user-facing contracts.

## 4. Conventions

- **Dependency direction**: this crate may use foundational dependencies such as `chrono`, `uuid`, `thiserror`, `tokio::sync`, and `async-trait`, but not other workspace crates.
- **Naming**: stable identities use `*Id` tuple structs around `Uuid`; durable process definitions use `*Spec`; runtime snapshots use `*Status`.
- **Trait shape**: async ports use `#[async_trait]`, require `Send + Sync`, and return `Result<T, PortError>` over domain types.
- **Defaults**: domain defaults encode common behavior (`Direct`, `Tied`, enabled restart, `Term` shutdown). Adapters should reuse these defaults rather than duplicating values.
- **Comments**: module comments describe architecture boundaries; inline comments are reserved for non-obvious lifecycle, safety, or compatibility details.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-core` after changes in this package.
