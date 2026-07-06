# AGENTS.md

## 1. Overview

`my-supervisor-application` owns transport-agnostic use cases for processes, jobs, logs, daemon status, config reload, scheduling, and shutdown. Host adapters call `OperationsFacade` rather than duplicating business rules.

## 2. Ownership Map

### Stable Ownership Boundaries

- **Facade behavior boundary**: Start in `src/facade.rs` when changing process commands, job commands, bootstrap, scheduler loop, daemon status, reload, or shutdown behavior. It owns in-memory runtime state and coordinates repository, lifecycle, scheduler, registrar, log, and config ports; preserve transport-agnostic signatures.
- **Dependency injection boundary**: Start in `src/deps.rs` when changing required external capabilities or daemon metadata. It owns the `AppDeps` trait-object bundle assembled by hosts; verify every host builder provides the new dependency.
- **Application error boundary**: Start in `src/error.rs` when changing user-visible failure semantics. `AppError::code()` and `http_status()` are consumed by HTTP and Tauri adapters; preserve stable codes unless all documented clients are updated.
- **Job runner boundary**: Start in `src/runner.rs` when changing how jobs become transient process executions. It owns `JobRunner` implementation, in-flight/final run persistence, and run events; preserve `Job` to detached Direct `ProcessSpec` conversion.

### Active Change Routes

- **SystemRegistered conversion route**: Within **Facade behavior boundary**, start in `OperationsFacade::convert_process` for Direct/SystemRegistered changes. Keep stop, unregister, register-before-save, rollback, and optional `auto_start` semantics aligned with macOS launchd registration.
- **Scheduler orchestration route**: Within **Facade behavior boundary**, start in `run_scheduler_loop`, `on_schedule_tick`, and `spawn_run` when changing scheduled job behavior. Preserve overlap guards and skipped-run persistence.

## 3. Core Behaviors & Patterns

- **Single use-case entry point**: HTTP routes, Tauri commands, and future transports call `OperationsFacade`; process, job, and daemon behavior belongs here, not in adapters.
- **Cfg-free adapter bundle**: `AppDeps` stores capabilities as `Arc<dyn Trait>`. Host crates select concrete adapters, while this crate stays free of platform or transport conditionals.
- **Direct versus SystemRegistered flow**: Direct processes update the `runtime` map after `spawn_tied` or `spawn_detached`; SystemRegistered processes call the registrar and query status from the OS boundary. Restart is a no-op result for SystemRegistered because the OS owns restart.
- **Job lifecycle**: jobs are validated, cycle-checked, persisted, then registered with the scheduler. Manual and scheduled runs go through `spawn_run`, emit domain events, call `JobRunner`, and clear the overlap guard when complete.
- **Bootstrap sequence**: hosts call `bootstrap()` after assembly to load config, arm scheduled jobs, and autostart flagged processes; hosts spawn `run_scheduler_loop()` separately.

## 4. Conventions

- **No transport types**: public facade methods return domain or application view types, never axum, Tauri, reqwest, or shared DTO types.
- **Guard before mutation**: validate empty commands, duplicate names, dependency cycles, and running-state conflicts before persisting, registering, or spawning.
- **Sorting**: list methods sort by user-visible stable keys such as `name` before returning.
- **Best-effort side effects**: cleanup, unregister, shutdown, event send, and autostart failures are ignored or logged only when they must not mask the authoritative operation result.
- **Event level**: `DomainEvent` variants describe domain changes; wire event strings belong in transport adapters.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-application` after changes in this package.
