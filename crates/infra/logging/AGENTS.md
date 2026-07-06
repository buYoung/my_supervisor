# AGENTS.md

## 1. Overview

`my-supervisor-infra-logging` provides the current in-memory `LogSink` implementation for process and job-run output. It keeps bounded history and live broadcast channels per source.

## 2. Folder Structure

- `src/lib.rs`: `InMemoryLogSink`, per-source channel state, snapshot logic, process log methods, and run log methods.

## 3. Core Behaviors & Patterns

- **Separate process and run channels**: process logs are keyed by process name, while job run logs are keyed by `JobRunId`.
- **Bounded ring buffers**: every channel uses a `VecDeque` capped by `RING_CAPACITY`; new lines evict the oldest line before broadcasting.
- **Live broadcast plus snapshot**: append methods push into the buffer and broadcast over a per-channel `broadcast::Sender`; tail methods return filtered snapshots from the buffer.
- **Lazy channel creation**: append and subscribe paths create channels on first use, allowing consumers to subscribe before any log line exists.
- **Since filtering**: process tails can filter by timestamp before applying the tail limit.

## 4. Conventions

- **Capacity constants**: keep `RING_CAPACITY` and `BROADCAST_CAPACITY` explicit near the implementation.
- **Mutex boundaries**: lock only the relevant map while locating or mutating a channel; avoid holding locks across awaited calls.
- **Default empty reads**: missing channels return empty/default tails rather than errors.
- **Run API symmetry**: keep `append_run`, `tail_run`, and `subscribe_run` behavior aligned with process log methods where the port allows it.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-infra-logging` after changes in this package.
