pub mod app;
pub mod background;
pub mod channels;
pub mod config;
pub mod hooks;
pub mod services;
pub mod typedb;

use dog_axum::AxumApp;
use serde_json::Value;
pub use services::FleetParams;
use std::sync::Arc;

pub async fn build() -> anyhow::Result<AxumApp<Value, FleetParams>> {
    let mut builder = app::build_builder().await?;

    let state = builder
        .get::<Arc<typedb::TypeDBState>>("typedb")
        .ok_or(anyhow::anyhow!("TypeDBState not found"))?;

    // Initialize background system
    let background_system = Arc::new(background::BackgroundSystem::new().await?);

    // Pass it to configure BEFORE building the app
    let _svcs = services::configure(
        &mut builder,
        Arc::clone(&state),
        Arc::clone(&background_system),
    )?;

    // Build the app (moves the builder)
    let dog_app = builder.build();
    let mut ax = dog_axum::axum(dog_app.clone())
        .use_service("/vehicles", _svcs.vehicles)
        .use_service("/deliveries", _svcs.deliveries)
        .use_service("/operations", _svcs.operations)
        .use_service("/employees", _svcs.employees)
        .use_service("/tomtom", _svcs.tomtom)
        .use_service("/jobs", _svcs.jobs)
        .use_service("/rules", _svcs.rules)
        .use_service("/certifications", _svcs.certifications)
        .service("/health", || async { "ok" })
        .service("/config", || async {
            let key = std::env::var("TOMTOM_API_KEY").unwrap_or_default();
            format!("{{\"tomtomApiKey\":\"{}\"}}", key)
        });

    // Start background system with built app
    background_system.start(dog_app).await?;

    // Add CORS middleware to allow browser requests
    ax.router = ax
        .router
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .fallback_service(tower_http::services::ServeDir::new("static"));

    Ok(ax)
}
