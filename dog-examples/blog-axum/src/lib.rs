mod app;
mod hooks;
mod channels;
mod services;

use std::sync::Arc;

use anyhow::Result;
use dog_axum::AxumApp;
use serde_json::Value;

use crate::services::BlogParams;

pub fn build() -> Result<AxumApp<Value, BlogParams>> {
    let ax = app::relay_app()?;
    let state = Arc::new(services::BlogState::default());

    hooks::global_hooks(ax.app.as_ref());
    channels::configure(ax.app.as_ref())?;

    let posts = services::configure(ax.app.as_ref(), Arc::clone(&state))?;

    let ax = ax
        .use_service("/posts", posts)
        .service("/health", || async { "ok" });

    Ok(ax)
}
