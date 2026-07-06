# AGENTS.md

## 1. Overview

`my-supervisor-application` owns the transport-agnostic use cases for processes, jobs, logs, daemon status, config reload, scheduling, and shutdown. Every host adapter calls `OperationsFacade` instead of duplicating business rules.

## 2. Folder Structure

- `src/lib.rs`: public exports for host and adapter crates.
- `src/deps.rs`: `AppDeps` trait-object bundle and `DaemonMeta` runtime metadata.
- `src/facade.rs`: main use-case entry point, in-memory runtime registry, job orchestration, scheduler loop, bootstrap, and shutdown behavior.
- `src/runner.rs`: `ProcessJobRunner`, which turns a `Job` into a transient Direct-mode `ProcessSpec`.
- `src/error.rs`: application error taxonomy, stable error codes, HTTP status hints, and port-error conversions.
- `src/events.rs`: domain events broadcast by facade operations and mapped by HTTP/Tauri adapters.
- `src/views.rs`: application view structs returned before DTO conversion.
- `src/registrar_null.rs`: no-op service registrar for hosts without SystemRegistered support.

## 3. Core Behaviors & Patterns

- **Single use-case entry point**: HTTP routes, Tauri commands, and future transports should call `OperationsFacade`. Do not place process, job, or daemon behavior in transport adapters.
- **Injected adapter bundle**: `AppDeps` contains all external capabilities as `Arc<dyn Trait>` values. Host crates select concrete adapters; this crate stays `#[cfg]`-free and only talks to ports.
- **Direct vs SystemRegistered process flow**: Direct processes are tracked in the `runtime` map after `spawn_tied` or `spawn_detached`; SystemRegistered processes call the registrar and derive status from `query_status`. Restart returns a no-op result for SystemRegistered processes because the OS owns restart behavior.
- **Transactional conversion**: `convert_process` stops the current mode best-effort, unregisters prior SystemRegistered traces, registers the new mode before persistence, and rolls back a new registration if saving fails.
- **Job lifecycle**: jobs are validated, cycle-checked, persisted, then registered with the scheduler. Manual and scheduled runs go through `spawn_run`, which marks the job as running, emits events, calls `JobRunner`, and removes the overlap guard when finished.
- **Failure mapping**: port errors convert into `AppError`; `AppError::code()` and `http_status()` are the canonical boundary data used by HTTP and Tauri adapters.
- **Bootstrap flow**: hosts call `bootstrap()` after assembly to load config into repositories, arm scheduled jobs, and autostart flagged processes. Scheduler driving is a separate host-spawned loop via `run_scheduler_loop()`.

## 4. Conventions

- **No transport types**: public facade methods return domain or application view types and must not expose axum, Tauri, reqwest, or DTO types.
- **Guards before mutation**: validate empty commands, duplicate names, dependency cycles, and running-state conflicts before persisting or spawning.
- **Sorting at query boundaries**: list methods sort by stable user-visible keys (`name`) before returning results.
- **Best-effort side effects**: cleanup, unregister, shutdown, and event broadcast failures are intentionally ignored or logged when they must not mask the authoritative operation result.
- **Event names stay domain-level**: `DomainEvent` variants describe process/job changes; wire event string names belong in adapters.
- **Constants name capacity/window semantics**: values such as `EVENT_CHANNEL_CAPACITY` and `RECENT_RUNS_WINDOW` should remain near the owning facade behavior.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-application` after changes in this package.
