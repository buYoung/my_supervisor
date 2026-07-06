# AGENTS.md

## 1. Overview

`my-supervisor-app-cli` implements the `msv` command-line client over the operations API. It reuses shared DTOs and daemon defaults instead of embedding domain logic.

## 2. Ownership Map

### Stable Ownership Boundaries

- **Command dispatch boundary**: Start in `src/main.rs` when changing command names, arguments, output modes, table rendering, or follow-log behavior. It owns CLI user interaction and delegates API calls to `Client`.
- **HTTP client boundary**: Start in `src/client.rs` when changing request paths, JSON decoding, error envelope handling, WebSocket base derivation, or exit-code mapping. It owns the transport behavior for CLI commands.
- **Config add boundary**: Start in `add_from_config` in `src/main.rs` when changing how a TOML file registers processes and jobs through the API. It must preserve shared config DTO reuse.

### Active Change Routes

- **Job command route**: Within **Command dispatch boundary**, start in `JobCmd` and matching `Client` methods when changing job listing, triggering, or run history. Keep table and JSON outputs backed by the same `shared` DTO responses.
- **Error exit route**: Within **HTTP client boundary**, start in `CliError` and `Client::send` when changing CLI failure behavior. Preserve `process_not_found` exit 2 and daemon connection failure exit 3 unless the documented convention changes.

## 3. Core Behaviors & Patterns

- **Thin API client**: commands call HTTP/WS endpoints exposed by daemon or desktop devBridge; no `core` or `application` types are used.
- **Shared DTO decoding**: request and response bodies use `my_supervisor_shared` types, so API contract drift is compile-visible.
- **Output split**: each command supports JSON output through the raw DTO and table/text output through local presentation helpers.
- **Log follow**: `logs -f` seeds from REST tail, then follows the process log WebSocket derived from the configured base URL.
- **Exit-code mapping**: normalized API errors and transport failures become `CliError` variants carrying process exit codes.

## 4. Conventions

- **Clap shape**: command enums stay close to the external command tree; subcommands own their specific arguments.
- **Presentation helpers**: keep label formatting helpers such as `process_state_label` and `run_state_label` in CLI code, not shared DTOs.
- **Client paths**: construct API paths in `Client` methods so dispatch code does not duplicate URLs.
- **JSON first**: when adding a command, make JSON output use the DTO directly and table output derive from that same value.

## 5. Working Agreements

See root `/AGENTS.md` for common working agreements.

Package-local verification: run `cargo check -p my-supervisor-app-cli` after changes in this package.
