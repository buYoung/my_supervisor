//! `my-supervisor-core` — domain entities and port traits. Depends on no other
//! workspace crate; carries no wire-format or OS-specific concern (DD-017/018).

pub mod domain;
pub mod ports;

pub use domain::*;
