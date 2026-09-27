#![doc = include_str!("../README.md")]

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
