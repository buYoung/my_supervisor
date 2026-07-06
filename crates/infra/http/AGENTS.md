# AGENTS.md

## 1. Overview

`my-supervisor-infra-http` exposes the operations facade as an axum REST and WebSocket API. It decodes requests, calls `OperationsFacade`, and maps domain/application values to shared DTOs.

## 2. Ownership Map

### Stable Ownership Boundaries

- **Route contract boundary**: Start in `src/lib.rs` when changing public HTTP or WebSocket paths. `build_router()` owns the `/api/v1` route manifest consumed by CLI, desktop devBridge, and browser clients; preserve alignment with `docs/API.md`.
- **Handler boundary**: Start in `src/handlers.rs` when changing request decoding, query defaults, status codes, or facade method selection. Handlers own transport behavior only and must not duplicate domain rules.
- **Mapping boundary**: Start in `src/mapping.rs` when changing domain/application to DTO translation or request DTO to domain conversion. It is the shared conversion home for HTTP routes and Tauri invoke handlers.
- **Streaming boundary**: Start in `src/ws.rs` when changing process logs, run logs, or global event streams. It owns upgrade detection, broadcast forwarding, lag behavior, and event wire envelopes.
- **HTTP error boundary**: Start in `src/error.rs` when changing error responses. It owns `AppError` to status plus `ErrorBody` envelope conversion.

### Active Change Routes

- **Convert route route**: Within **Route contract boundary**, start in `build_router`, `convert_process`, and `management_mode_to_dto` when changing Direct/SystemRegistered conversion over HTTP. Keep the CLI, Tauri invoke, and shared DTO names synchronized.
- **Log follow route**: Within **Streaming boundary**, start in `process_logs`, `run_logs`, and `forward_logs` when changing tail/follow behavior. Preserve JSON tail responses for non-upgrade requests and `log.dropped` frames for lagged receivers.

## 3. Core Behaviors & Patterns

- **Thin route adapters**: routes parse path/query/body data, call the facade, and map results. Domain decisions stay in `application`.
- **Central mapping**: all domain/application and shared DTO conversion lives in `mapping.rs`; handlers and WebSocket code reuse it.
- **Uniform errors**: `HttpError` converts `AppError` into an HTTP status and `shared::error::ErrorBody` with the stable application code.
- **Dual log endpoint**: `GET /api/v1/processes/{name}/logs` serves either a JSON tail or a WebSocket live stream depending on upgrade headers.
- **Broadcast resilience**: log streams emit `log.dropped` control frames on lag; global event streams skip lagged events and close cleanly when the channel closes.

## 4. Conventions

- **Return shapes**: use `Result<Response, HttpError>` when bodies vary, `Result<StatusCode, HttpError>` for empty success responses, and plain `StatusCode` only for handlers that cannot surface application errors.
- **Boundary defaults**: apply HTTP defaults such as `force=false`, `tail=100`, and capped run limits in handlers before calling the facade.
- **Bad UUID behavior**: malformed run IDs map to `run_not_found`, matching the external API contract.
- **Path style**: routes use plural resources and action suffixes already present in the manifest (`/start`, `/stop`, `/restart`, `/trigger`).
- **CORS assumption**: permissive CORS is tied to loopback, single-user, no-auth usage; revisit it if binding or auth assumptions change.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-infra-http` after changes in this package.
