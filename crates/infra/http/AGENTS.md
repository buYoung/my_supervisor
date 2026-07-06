# AGENTS.md

## 1. Overview

`my-supervisor-infra-http` exposes the operations facade as an axum REST and WebSocket API. It is a thin transport adapter: decode requests, call `OperationsFacade`, and map domain/application values to shared DTOs.

## 2. Folder Structure

- `src/lib.rs`: `assemble()`, router construction, route manifest, CORS layer, and `Assembled` host artifact.
- `src/handlers.rs`: REST handler functions and query extractors.
- `src/ws.rs`: process log, run log, and global event WebSocket handling.
- `src/mapping.rs`: centralized domain/application ↔ shared DTO conversion.
- `src/error.rs`: `AppError` to HTTP status and `ErrorBody` envelope conversion.

## 3. Core Behaviors & Patterns

- **Thin route adapters**: handlers parse path/query/body data, call facade methods, and map results. Domain rules belong in `application`, not here.
- **Central mapping boundary**: all DTO/domain conversion lives in `mapping.rs`; handlers and WebSocket code reuse it rather than building DTOs inline.
- **Uniform error envelope**: `HttpError` converts `AppError` into HTTP status plus `shared::error::ErrorBody`, preserving the stable application error code.
- **Dual REST/WS log route**: `GET /api/v1/processes/{name}/logs` returns JSON for normal requests and upgrades to a live log stream when a WebSocket upgrade is present.
- **Broadcast resilience**: log streams emit `log.dropped` control frames on lag; the global event stream skips lagged domain events and exits on channel closure.
- **Route manifest as contract**: `build_router()` enumerates `/api/v1` routes in one place and stays aligned with `docs/API.md`.

## 4. Conventions

- **Handler return shape**: handlers return `Result<Response, HttpError>` when bodies vary, `Result<StatusCode, HttpError>` for empty success responses, and plain `StatusCode` only when errors are not surfaced.
- **Query defaults at boundary**: HTTP defaults such as `force=false`, `tail=100`, and capped run limits are applied in handlers before calling the facade.
- **Bad run IDs**: malformed run UUIDs map to `run_not_found`, matching the external API behavior.
- **Route naming**: route paths use plural resources and action suffixes (`/start`, `/stop`, `/restart`, `/trigger`) already defined in the manifest.
- **CORS assumption**: permissive CORS is tied to loopback, single-user, no-auth usage. Revisit it if binding or auth assumptions change.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-infra-http` after changes in this package.
