# AGENTS.md

## 1. Overview

`my-supervisor-app-desktop` is the Tauri desktop host and bundled React operations console. It embeds the shared daemon runtime in-process and exposes the same facade through Tauri commands plus a test-only loopback devBridge.

## 2. Folder Structure

- `src/main.rs`: Tauri setup, in-process runtime assembly, invoke handlers, tray behavior, close-to-tray handling, and devBridge serving.
- `build.rs`: Tauri build integration.
- `tauri.conf.json`: Tauri application configuration and capabilities entry points.
- `gen`: generated Tauri schemas and capabilities; treat as generated output.
- `icons`: desktop app icon assets.
- `ui`: React/Vite frontend.
    - `src/services`: transport interface, invoke adapter, HTTP/WS adapter, wire DTOs, wire-to-UI mapping, and polling hooks.
    - `src/features`: operational screens for processes, jobs, logs, daemon status, and settings.
    - `src/components/ui`: small reusable UI primitives.
    - `src/shared`: frontend-only types, mock data, and theme tokens.

## 3. Core Behaviors & Patterns

- **In-process host**: desktop calls `build_runtime()` from the daemon crate and does not spawn a separate `msv-daemon` process.
- **Transport parity**: Tauri invoke handlers and the devBridge HTTP router call the same `OperationsFacade` instance. Keep handlers thin and use `infra_http::mapping` for DTO conversion.
- **Serializable command errors**: invoke handlers convert `AppError` into `{ code, message }`, matching the API error-code contract without exposing Rust error types to the UI.
- **Close-to-tray lifecycle**: close requests hide the window so supervision continues; explicit tray quit exits the application.
- **UI transport abstraction**: feature views depend only on `OperationsClient`. Runtime detection selects invoke inside Tauri and HTTP/WebSocket otherwise.
- **Shared frontend mapping**: both UI transports pass snake_case wire DTOs through `wire-mapping.ts` before views see camelCase frontend shapes.
- **Polling and live logs**: `usePolledResource()` owns refresh/error state for status screens; log views seed from a tail and then subscribe over Tauri events or WebSocket.

## 4. Conventions

- **Invoke command names**: Rust command functions use the `cmd_*` prefix and mirror operations API names.
- **Handler shape**: each Tauri command decodes arguments, calls the facade, maps output, and returns `CmdResult<T>`; do not add domain decisions there.
- **Frontend services**: add operations to `OperationsClient` first, then implement both `createInvokeClient()` and `createHttpClient()` using `wire-mapping.ts`.
- **Frontend state names**: hooks return `{ data, isLoading, errorMessage, refresh }`; booleans use `is/has/can/should` prefixes.
- **View composition**: feature files use `Panel`, `PanelHeader`, `DataTable`, `Badge`, `Button`, and `IconButton` primitives; icons come from `lucide-react`.
- **Wire naming split**: backend/shared DTOs remain snake_case; UI-facing types remain camelCase and are never pushed back onto the wire.
- **Generated files**: avoid manual edits under `gen` unless the generation source requires it.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-app-desktop` after Rust changes in this package; run `pnpm --dir ui typecheck` after UI changes.
