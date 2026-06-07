//! Library template created with FerrisUp

//! dog-axum: Axum adapter for DogRS.
//!
//! This crate will expose helpers to build Axum routers
//! from DogRS services and apps.

pub mod app;
mod error;
pub mod middlewares;
pub mod oauth;
pub mod params;
pub mod rest;
pub mod state;
pub use error::DogAxumError;
pub use state::DogAxumState;

pub use app::{axum, AxumApp};
