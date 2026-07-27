//! `my-supervisor-platform-macos` — macOS adapters. Foundation ships the
//! Direct-mode `MacLifecycle` + `UnixShutdown`; child 06 adds the launchd
//! `LaunchdAgentProcess` (`ProcessServiceRegistrar`) to this same crate.

mod guards;
mod hook;
mod launchd;
mod lifecycle;
mod notification;
mod owner;
pub mod process_identity;
mod shutdown;
mod signals;
mod spawn;

pub use hook::HookDelivery;
pub use launchd::{
    LaunchdAgentProcess, LaunchdTestControls, SupervisorLaunchAgent, SupervisorServiceState,
    SupervisorServiceStatus, SUPERVISOR_LABEL,
};
pub use lifecycle::MacLifecycle;
pub use notification::NotificationCenterDelivery;
pub use owner::{
    ensure_private_directory, read_private_file, write_private_file_atomic, OwnerLock,
};
pub use shutdown::UnixShutdown;
pub use spawn::{DetachedHelperPaths, DetachedTestControls};
