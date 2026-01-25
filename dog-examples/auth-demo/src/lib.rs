mod app;
mod hooks;
mod channels;
mod services;

use anyhow::Result;
use dog_axum::AxumApp;
use serde_json::Value;

use crate::services::AuthDemoParams;

pub fn build() -> Result<AxumApp<Value, AuthDemoParams>> {
    let ax = app::auth_app()?;

    hooks::global_hooks(ax.app.as_ref());
    channels::configure(ax.app.as_ref())?;

    let svcs = services::configure(ax.app.as_ref())?;

    let ax = ax
        .use_service("/messages", svcs.messages)
        .use_service("/users", svcs.users)
        .use_service("/auth", svcs.auth_svc)
        .service("/health", || async { "ok" });

    Ok(ax)
}
