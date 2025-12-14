//! Library template created with FerrisUp

//! dog-core: framework-agnostic core for DogRS.

pub mod app;
pub mod config;
pub mod hooks;
pub mod registry;
pub mod service;
pub mod tenant;
pub mod events;
pub mod errors;

pub use app::{DogApp, ServiceCaller};
pub use config::{DogConfig, DogConfigSnapshot };
pub use hooks::{
    DogAfterHook, DogAroundHook, DogBeforeHook, DogErrorHook, HookContext, Next, ServiceHooks, HookResult,
};
pub use registry::DogServiceRegistry;
pub use service::{DogService, ServiceCapabilities, ServiceMethodKind};
pub use tenant::{TenantContext, TenantId};
pub use events::{DogEventHub, ServiceEventKind, ServiceEventData, method_to_standard_event, };
pub use errors::{DogError, ErrorKind};
