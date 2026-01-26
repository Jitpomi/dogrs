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

    let svcs = services::configure(ax.app.as_ref())?;

    let ax = ax
        .use_service("/messages", svcs.messages)
        .use_service("/users", svcs.users)
        .use_service("/auth", svcs.auth_svc)
        .use_service("/oauth", svcs.oauth)
        .service("/health", || async { "ok" });

    Ok(ax)
}
