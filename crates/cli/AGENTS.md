# AGENTS.md

## 1. Overview

`my-supervisor-app-cli` implements the `msv` operations client. It talks to the daemon API over HTTP/WebSocket and reuses shared DTOs; it does not embed core behavior.

## 2. Folder Structure

- `src/main.rs`: clap command model, dispatch, table/JSON output, config import, log follow, and process exit handling.
- `src/client.rs`: HTTP request wrapper, API methods, error-envelope mapping, daemon-down detection, and WebSocket base derivation.

## 3. Core Behaviors & Patterns

- **Thin client boundary**: CLI commands call `Client` methods over `/api/v1`; process/job semantics remain in the daemon facade.
- **Shared DTO reuse**: API bodies and responses use `my_supervisor_shared` DTOs so contract changes break compilation.
- **Exit-code mapping**: `CliError` maps `process_not_found` to exit 2, connection failures to exit 3, and other failures to exit 1.
- **Output mode split**: commands use table output by default and pretty JSON when `-o json` is selected.
- **Config import**: `Add -c` reads `FileConfig` and posts each process/job DTO through the same API used by other clients.
- **Log follow**: `logs -f` prints the REST tail first, then follows the WebSocket stream derived from the configured base URL.

## 4. Conventions

- **Command naming**: top-level subcommands are concise operational verbs or nouns (`ps`, `start`, `stop`, `restart`, `logs`, `reload`, `daemon`, `job`).
- **Client wrapper**: add new API calls to `client.rs` as typed methods and keep raw request construction behind `send()`/`get_json()`.
- **Error messages**: user-facing CLI failures come from `CliError::message()` and preserve daemon envelope messages when available.
- **URL handling**: normalize base URLs by trimming trailing slashes and derive `ws://` or `wss://` with `ws_base()`.
- **No domain imports**: keep CLI code on shared DTOs and daemon defaults, not `core` domain types.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-app-cli` after changes in this package.
