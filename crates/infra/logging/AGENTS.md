# AGENTS.md

## 1. Overview

`my-supervisor-infra-logging` implements the in-memory `LogSink` adapter. It keeps bounded process and job-run log buffers and broadcasts newly captured lines to live subscribers.

## 2. Ownership Map

### Stable Ownership Boundaries

- **Log channel boundary**: Start in `Channel` and `snapshot` when changing buffering, truncation, or broadcast behavior. It owns ring capacity, live fan-out, and tail semantics consumed by HTTP, Tauri, CLI, and UI log views.
- **Process log boundary**: Start in the `LogSink` process methods when changing per-process append, tail, or subscribe behavior. Process logs are keyed by process name and feed `/processes/{name}/logs`.
- **Run log boundary**: Start in the `LogSink` run methods when changing per-job-run log behavior. Run logs are keyed by `JobRunId` and feed job run WebSocket subscriptions.

## 3. Core Behaviors & Patterns

- **Bounded ring buffer**: each source uses a `VecDeque` capped at `RING_CAPACITY`; pushing a new line drops the oldest line when full.
- **Live broadcast per source**: each channel has its own `broadcast::Sender`, so subscribers receive only new lines for the selected process or run.
- **Snapshot filtering**: tail reads filter by `since`, then apply the requested limit and report whether older matching lines were truncated.
- **Lazy channel creation**: appending or subscribing creates a channel on demand, allowing follow-before-output flows.

## 4. Conventions

- **Separate maps**: keep process logs and run logs in separate mutex-protected maps because they use different keys and API routes.
- **No disk retention here**: on-disk rotation, archives, and durable log retention are outside this adapter's current responsibility.
- **Silent send failure**: ignore broadcast send errors because they only mean there are no live subscribers.
- **Capacity constants**: keep `RING_CAPACITY` and `BROADCAST_CAPACITY` near the adapter implementation that owns their semantics.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-infra-logging` after changes in this package.
