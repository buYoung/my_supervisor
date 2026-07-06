//! `my-supervisor-platform-macos` — macOS adapters. Foundation ships the
//! Direct-mode `MacLifecycle` + `UnixShutdown`; child 06 adds the launchd
//! `LaunchdAgentProcess` (`ProcessServiceRegistrar`) to this same crate.

mod launchd;
mod lifecycle;
mod shutdown;
mod signals;
mod spawn;

pub use launchd::LaunchdAgentProcess;
pub use lifecycle::MacLifecycle;
pub use shutdown::UnixShutdown;
