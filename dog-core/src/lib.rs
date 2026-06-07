//! dog-core: framework-agnostic core for DogRS.

pub mod app;
pub mod config;
pub mod errors;
pub mod events;
pub mod hooks;
pub mod registry;
pub mod service;
pub mod tenant;

#[cfg(feature = "adapters")]
pub mod adapters;

// Branch: DogAppBuilder, ServiceHandle, ServiceBuilderHandle (builder-pattern refactor)
// Main: ErrorValue, DogValue re-exports (format-agnostic serde PR)
pub use app::{DogApp, DogAppBuilder, ServiceBuilderHandle, ServiceCaller, ServiceHandle};
pub use config::{DogConfig, DogConfigSnapshot};
pub use errors::{DogError, DogResult, ErrorKind, ErrorValue};
pub use events::{method_to_standard_event, DogEventHub, ServiceEventData, ServiceEventKind};
pub use hooks::{
    DogAfterHook, DogAroundHook, DogBeforeHook, DogErrorHook, HookContext, HookResult, Next,
    ServiceHooks,
};
pub use registry::DogServiceRegistry;
pub use service::{DogService, ServiceCapabilities, ServiceMethodKind};
pub use tenant::{TenantContext, TenantId};
#[cfg(all(feature = "serde", not(feature = "json")))]
pub use errors::DogValue;
