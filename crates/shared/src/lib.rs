//! `my-supervisor-shared` — wire DTOs (REST + WS) and the TOML config schema.
//! Snake_case per `docs/API.md` §4. Imported by `infra/http` and `app/cli` so
//! contract changes are caught at compile time.

pub mod api;
pub mod config;
pub mod error;
pub mod events;
