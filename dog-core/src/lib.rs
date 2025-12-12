//! Library template created with FerrisUp

//! dog-core: framework-agnostic core for DogRS.

pub mod tenant;
pub mod service;

pub use tenant::{TenantContext, TenantId};
pub use service::DogService;
