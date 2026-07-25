//! `my-supervisor-platform-macos` — macOS adapters. Foundation ships the
//! Direct-mode `MacLifecycle` + `UnixShutdown`; child 06 adds the launchd
//! `LaunchdAgentProcess` (`ProcessServiceRegistrar`) to this same crate.

mod launchd;
mod lifecycle;
pub mod process_identity;
mod shutdown;
mod signals;
mod spawn;

pub use launchd::{LaunchdAgentProcess, LaunchdTestControls};
pub use lifecycle::MacLifecycle;
pub use shutdown::UnixShutdown;
pub use spawn::{DetachedHelperPaths, DetachedTestControls};
