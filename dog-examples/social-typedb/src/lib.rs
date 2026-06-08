mod app;
mod channels;
mod hooks;
mod services;
mod typedb;

use dog_axum::AxumApp;
use serde_json::Value;
pub use services::SocialParams;
use std::sync::Arc;

pub async fn build() -> anyhow::Result<AxumApp<Value, SocialParams>> {
    let mut builder = app::build_builder().await?;

    let state = builder
        .get::<Arc<typedb::TypeDBState>>("typedb")
        .ok_or(anyhow::anyhow!("TypeDBState not found"))?;

    let svcs = services::configure(&mut builder, Arc::clone(&state))?;

    let mut ax = dog_axum::axum(builder.build())
        .use_service("/persons", svcs.persons)
        .use_service("/organizations", svcs.organizations)
        .use_service("/groups", svcs.groups)
        .use_service("/posts", svcs.posts)
        .use_service("/comments", svcs.comments)
        .service("/health", || async { "ok" });

    // Add CORS middleware to allow browser requests
    ax.router = ax
        .router
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .fallback_service(tower_http::services::ServeDir::new(
            "dog-examples/social-typedb/static",
        ));

    Ok(ax)
}
