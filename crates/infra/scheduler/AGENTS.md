# AGENTS.md

## 1. Overview

`my-supervisor-infra-scheduler` implements the scheduler port with Tokio timers and broadcasts scheduled job fire events. It handles timed triggers; dependency-trigger propagation belongs to application-level observers.

## 2. Folder Structure

- `src/lib.rs`: `TokioScheduler`, trigger parsing, next-run calculation, timer registration, timer abort, and schedule-event subscription.

## 3. Core Behaviors & Patterns

- **One timer per job**: `register()` aborts any existing timer for the job before creating a new one, so updates replace prior schedules cleanly.
- **Cron validation up front**: cron triggers are parsed during registration and fail fast with `SchedulerError::InvalidCron`.
- **DependsOn is not timed**: dependency triggers register successfully without spawning a timer; application code reacts to completed upstream runs.
- **Interruptible far-future sleeps**: `sleep_until()` caps each sleep by `MAX_SLEEP` so timer tasks can react to aborts instead of sleeping indefinitely.
- **Pure next-run calculation**: `next_for()` handles cron, interval, one-shot, and dependency triggers and is reused by both timer tasks and facade status views.

## 4. Conventions

- **Timer ownership**: store spawned timer `JoinHandle`s in the `timers` map and cancel through `abort()`.
- **Broadcast events**: scheduled fires publish `ScheduleEvent { job_name, scheduled_at }` over the scheduler sender; job execution is not started in this crate.
- **Time source boundary**: scheduler internals use `Utc::now()` for timer driving; application view code passes `SystemClock` time into `next_run()`.
- **No persistence**: scheduler state is volatile and rebuilt by `OperationsFacade::bootstrap()` from repositories.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-infra-scheduler` after changes in this package.
