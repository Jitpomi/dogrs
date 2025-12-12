//! Library template created with FerrisUp

//! dog-core: framework-agnostic core for DogRS.

pub mod tenant;
pub mod service;
pub mod hooks;
pub mod registry;
pub mod app;
pub mod config;

pub use tenant::{TenantContext, TenantId};
pub use service::DogService;
pub use hooks::{DogHook, HookContext, HookStage};
pub use registry::DogServiceRegistry;
pub use app::DogApp;
pub use config::DogConfig;

