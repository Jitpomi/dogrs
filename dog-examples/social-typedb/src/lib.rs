mod app;
mod hooks;
mod channels;
mod services;
mod typedb;

use std::sync::Arc;
use serde_json::Value;
use dog_axum::AxumApp;
pub use services::{SocialParams};

pub async fn build() -> anyhow::Result<AxumApp<Value, SocialParams>> {
    let ax = app::social_app()?;
    typedb::TypeDBState::setup_db(ax.app.as_ref()).await?;

    let state = ax.app.get::<Arc<typedb::TypeDBState>>("typedb").ok_or(anyhow::anyhow!("TypeDBState not found"))?;

    hooks::global_hooks(ax.app.as_ref());
    channels::configure(ax.app.as_ref())?;

    let svcs = services::configure(ax.app.as_ref(), Arc::clone(&state))?;

    let ax = ax
        .use_service("/persons", svcs.persons)

        .service("/health", || async { "ok" });

    Ok(ax)
}
