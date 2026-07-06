# AGENTS.md

## 1. Overview

`my-supervisor-core` defines the supervisor domain model and every port trait used by higher layers. It is the stable boundary that keeps process control, persistence, scheduling, config, and logging independent of concrete hosts and adapters.

## 2. Folder Structure

- `src/lib.rs`: exports the domain and port modules; do not add host or adapter wiring here.
- `src/domain`: pure domain entities and value types.
    - `process.rs`: Direct/SystemRegistered process definitions, lifecycle modes, shutdown and restart policy, child handles, and status snapshots.
    - `job.rs`: job definitions, run identity, trigger variants, overlap policy, dependency policy, run state, and trigger origin.
    - `log.rs`: log line and stream models used by process and job output flows.
    - `config.rs`: loaded configuration aggregate consumed by the application layer.
- `src/ports`: async traits and port error types implemented by adapters.
    - `repository.rs`: process registry and job/run persistence ports.
    - `lifecycle.rs`: Direct-mode spawn, probe, reaping, and transient job execution.
    - `registrar.rs`: SystemRegistered service manager boundary.
    - `scheduler.rs`, `log_sink.rs`, `config_source.rs`, `shutdown.rs`, `clock.rs`: remaining external boundaries.

## 3. Core Behaviors & Patterns

- **Ports and adapters boundary**: `core` owns trait definitions such as `LifecycleController`, `StateRepository`, `JobRepository`, `ProcessServiceRegistrar`, `Scheduler`, and `LogSink`; `application` composes these traits, while `infra/*`, `platform/*`, and `config` implement them. New external capabilities should enter through a focused port rather than by adding adapter imports to domain code.
- **Process mode split**: `ManagementMode::Direct` means the daemon owns spawn/probe/stop via `LifecycleController` and `ShutdownSignaler`; `ManagementMode::SystemRegistered` delegates lifecycle to `ProcessServiceRegistrar` keyed by `unit_name`. Keep mode-specific data on the enum and preserve this split through application and DTO mapping.
- **Jobs are transient process executions**: `Job` and `JobRun` intentionally model run-to-completion work separately from supervised `ProcessSpec`. Job execution uses `JobTrigger`, `TriggeredBy`, and terminal `JobRunState` rather than process runtime state.
- **Error containment**: port errors live under `ports::error` or per-port modules and describe backend failures without HTTP, Tauri, CLI, or UI concepts. Boundary layers convert these errors outward.
- **State lifecycle helpers**: small methods such as `JobRunState::is_terminal` encode lifecycle rules close to the enum. Add similar helpers near domain types when a rule is shared across layers.

## 4. Conventions

- **Dependency direction**: this crate may depend on lightweight foundational crates such as `chrono`, `uuid`, `thiserror`, `tokio::sync`, and `async-trait`, but not on other workspace crates.
- **Domain naming**: stable identities use `*Id` tuple structs around `Uuid` (`JobId`, `JobRunId`); runtime snapshots use `*Status`; durable definitions use `*Spec` or direct entity names (`ProcessSpec`, `Job`).
- **Trait signatures**: async ports use `#[async_trait]`, require `Send + Sync`, and return `Result<T, PortError>` with domain types, not wire DTOs.
- **Defaults**: domain defaults express the common behavior (`Direct`, `Tied`, `RestartPolicy::enabled = true`, `ShutdownSignal::Term`). Avoid duplicating default values in adapters; use the domain defaults during mapping.
- **Comments**: module comments explain architectural boundaries and design-decision intent. Inline comments are reserved for non-obvious lifecycle or safety behavior.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-core` after changes in this package.
