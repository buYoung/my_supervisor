//! `my-supervisor-application` — use cases over `core` ports (DD-017). Exposes a
//! transport-agnostic `OperationsFacade` that HTTP routes and Tauri invoke
//! handlers call identically (the parity precondition).

pub mod config_apply;
pub mod deps;
pub mod error;
pub mod events;
pub mod facade;
pub mod registrar_null;
pub mod runner;
pub mod views;

pub use deps::{AppDeps, DaemonMeta};
pub use error::{AppError, AppResult, ConflictReason, ResourceKind};
pub use events::{DomainEvent, PublishedEvent};
pub use facade::OperationsFacade;
pub use registrar_null::NullProcessServiceRegistrar;
pub use runner::ProcessJobRunner;
pub use views::{
    ConvertTarget, DaemonInfo, JobView, LogPage, RecoveryDiagnostic, RecoveryDiagnostics,
    RestartOutcome,
};
