mod app;
mod hooks;
mod channels;
mod services;
mod typedb;

use std::sync::Arc;
use serde_json::Value;
use dog_axum::AxumApp;
pub use services::SocialParams;

pub async fn build() -> anyhow::Result<AxumApp<Value, SocialParams>> {
    let ax = app::social_app()?;
    typedb::TypeDBState::setup_db(ax.app.as_ref()).await?;

    let state = ax.app.get::<Arc<typedb::TypeDBState>>("typedb").ok_or(anyhow::anyhow!("TypeDBState not found"))?;

    hooks::global_hooks(ax.app.as_ref());
    channels::configure(ax.app.as_ref())?;

    let svcs = services::configure(ax.app.as_ref(), Arc::clone(&state))?;

    let mut ax = ax
        .use_service("/persons", svcs.persons)
        .use_service("/organizations", svcs.organizations)
        .use_service("/groups", svcs.groups)
        .use_service("/posts", svcs.posts)
        .use_service("/comments", svcs.comments)

        .service("/health", || async { "ok" });

    // Add CORS middleware to allow browser requests
    ax.router = ax.router
        .layer(tower_http::cors::CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any))
        .fallback_service(tower_http::services::ServeDir::new("dog-examples/social-typedb/static"));

    Ok(ax)
}
