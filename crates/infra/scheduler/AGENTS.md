# AGENTS.md

## 1. Overview

`my-supervisor-infra-scheduler` implements the timed `Scheduler` adapter. It evaluates cron, interval, and one-shot triggers with Tokio timers and broadcasts schedule events to the application layer.

## 2. Ownership Map

### Stable Ownership Boundaries

- **Trigger timing boundary**: Start in `next_for`, `parse_cron`, and `next_cron` when changing how triggers compute their next fire time. It owns cron validation, interval arithmetic, one-shot expiration, and `DependsOn` non-timed behavior.
- **Timer lifecycle boundary**: Start in `TokioScheduler::register`, `abort`, and `unregister` when changing timer arming. It owns replacing prior timers and aborting handles for deleted or updated jobs.
- **Schedule event boundary**: Start in `Scheduler::subscribe` and the broadcast sender when changing how fired jobs reach `OperationsFacade::run_scheduler_loop`.

### Active Change Routes

- **Responsive sleep route**: Within **Timer lifecycle boundary**, start in `sleep_until` when changing long future waits. Preserve the capped sleep loop so far-future one-shots remain abort-responsive.

## 3. Core Behaviors & Patterns

- **Re-register replaces**: registering a job first aborts any existing timer for that name, then arms the new trigger.
- **Cron validates early**: cron expressions are parsed during registration so invalid schedules fail before persistence-dependent orchestration continues.
- **Dependency triggers are external**: `JobTrigger::DependsOn` returns no timer and emits no direct schedule event; dependency propagation belongs to run-completion observers.
- **Broadcast-driven loop**: timers send `ScheduleEvent`; the application layer decides overlap behavior and spawns runs.

## 4. Conventions

- **Pure next-run helper**: keep next-run computation in `next_for` so `next_run()` and timer loops share the same behavior.
- **Mutex-protected handles**: timer handles live in a `HashMap<String, JoinHandle<()>>` behind a mutex; abort before replacement.
- **Capacity naming**: event channel capacity constants should describe the resource they bound.
- **Error mapping**: scheduler-specific validation failures become `SchedulerError::InvalidCron`; backend failures use `SchedulerError::Backend`.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-infra-scheduler` after changes in this package.
