# AGENTS.md

## 1. Overview

`my-supervisor-app-desktop` is the Tauri desktop host and bundled React operations console. It embeds the shared daemon runtime in-process and exposes the same facade through Tauri commands plus a test-only loopback devBridge.

## 2. Ownership Map

### Stable Ownership Boundaries

- **Tauri host boundary**: Start in `src/main.rs` when changing runtime setup, invoke handlers, tray behavior, close-to-tray lifecycle, devBridge serving, or command error shape. It owns desktop host integration while keeping domain behavior in `OperationsFacade`.
- **Transport abstraction boundary**: Start in `ui/src/services/operations-client.ts` when changing frontend operation capabilities. Feature views depend on this interface, so both invoke and HTTP adapters must be updated together.
- **Invoke adapter boundary**: Start in `ui/src/services/invoke-client.ts` and matching Rust `cmd_*` functions when changing production desktop transport. Preserve command names, argument casing expectations, and shared wire mapping.
- **HTTP adapter boundary**: Start in `ui/src/services/http-client.ts` when changing standalone/browser transport or WebSocket log follow behavior. It must remain behaviorally equivalent to invoke for the same `OperationsClient` method.
- **Wire mapping boundary**: Start in `ui/src/services/wire-mapping.ts` and `wire-types.ts` when changing frontend DTO reconciliation. It owns snake_case wire to camelCase UI conversion and derived UI-only fields.
- **Feature view boundary**: Start in `ui/src/features/*` when changing screen behavior. Views should call service hooks/client methods and keep transport details out.

### Active Change Routes

- **DevBridge parity route**: Across **Tauri host boundary** and **HTTP adapter boundary**, start in `run_devbridge` and `createHttpClient` when changing test automation transport. Keep it using the same router/facade as invoke handlers.
- **Live log route**: Across **Invoke adapter boundary** and **HTTP adapter boundary**, start in `cmd_follow_logs`, `followProcessLogs`, and `mapLogLine` when changing live logs. Preserve tail seeding plus event/WebSocket follow semantics.
- **SystemRegistered UI route**: Across **Transport abstraction boundary** and **Wire mapping boundary**, start in `convertProcess` and management-mode mapping when changing Direct/SystemRegistered UI support.

## 3. Core Behaviors & Patterns

- **In-process runtime**: desktop calls `build_runtime()` from the daemon crate and does not spawn a separate `msv-daemon` process.
- **Transport parity**: Tauri invoke handlers and devBridge HTTP routes call the same `OperationsFacade` instance and use `infra_http::mapping` for DTO conversion.
- **Serializable command errors**: invoke handlers convert `AppError` into `{ code, message }`, matching the API error-code contract without exposing Rust errors to UI code.
- **Close-to-tray lifecycle**: close requests hide the window so supervision continues; tray quit exits the application.
- **Client abstraction**: feature views consume `OperationsClient`; runtime detection selects invoke inside Tauri and HTTP/WebSocket otherwise.
- **Polling and live updates**: `usePolledResource()` owns status refresh/error state; log views seed from a tail and then subscribe over Tauri events or WebSocket.

## 4. Conventions

- **Invoke names**: Rust command functions use the `cmd_*` prefix and mirror operations API names.
- **Handler shape**: each Tauri command decodes arguments, calls the facade, maps output, and returns `CmdResult<T>`; do not add domain decisions there.
- **Frontend service changes**: add operations to `OperationsClient` first, then implement both `createInvokeClient()` and `createHttpClient()` using `wire-mapping.ts`.
- **Hook state shape**: hooks return `{ data, isLoading, errorMessage, refresh }`; booleans use `is/has/can/should` prefixes.
- **UI composition**: feature views use shared primitives from `components/ui/primitives.tsx`; icons come from `lucide-react`.
- **Naming split**: backend/shared DTOs remain snake_case; UI-facing types remain camelCase and are never pushed back onto the wire.
- **Generated files**: avoid manual edits under `gen` unless changing the generation source.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-app-desktop` after Rust changes in this package; run `pnpm --dir ui typecheck` after UI changes.
