//! Library template created with FerrisUp

//! dog-axum: Axum adapter for DogRS.
//!
//! This crate will expose helpers to build Axum routers
//! from DogRS services and apps.

use axum::Router;

/// For now, just expose a minimal router constructor.
/// Later we’ll make this take a DogRS app/registry.
pub fn new_router() -> Router {
    Router::new()
}
