mod app;
mod auth;
mod channels;
mod config;
mod hooks;
mod services;

use anyhow::Result;
use dog_core::DogApp;
use dog_transport::http::DogHttpService;
use serde_json::Value;

pub use crate::services::AuthDemoParams;
pub use auth::oauth2::google::http::configure as configure_oauth;

pub async fn build() -> Result<(DogApp<Value, AuthDemoParams>, DogHttpService<Value, AuthDemoParams>)> {
    let (dog, http_service) = app::auth_app().await?;
    Ok((dog, http_service))
}
