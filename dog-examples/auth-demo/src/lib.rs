mod app;
mod auth;
mod config;
mod hooks;
mod channels;
mod services;

use anyhow::Result;
use dog_axum::AxumApp;
use serde_json::Value;

use crate::services::AuthDemoParams;

pub async fn build() -> Result<AxumApp<Value, AuthDemoParams>> {
    let ax = app::auth_app().await?;

    let ax = ax
        .service("/health", || async { "ok" });

    Ok(ax)
}
