pub mod typedb;
pub mod services;
pub mod hooks;
pub mod channels;
pub mod background;
pub mod config;
pub mod app;

use std::sync::Arc;
use serde_json::Value;
use dog_axum::AxumApp;
pub use services::FleetParams;

pub async fn build() -> anyhow::Result<AxumApp<Value, FleetParams>> {
    let ax = app::fleet_app()?;
    typedb::TypeDBState::setup_db(ax.app.as_ref()).await?;

    let state = ax.app.get::<Arc<typedb::TypeDBState>>("typedb").ok_or(anyhow::anyhow!("TypeDBState not found"))?;

    hooks::global_hooks(ax.app.as_ref());
    channels::configure(ax.app.as_ref())?;
    // Initialize background system and store in app state  
    let mut background_system = background::BackgroundSystem::new(Arc::new(ax.clone())).await?;
    background_system.start().await?;
    let background_system = Arc::new(background_system);
    ax.app.set("background_system", background_system.clone());

    let svcs = services::configure(ax.app.as_ref(), Arc::clone(&state))?;

    let mut ax = ax
        .use_service("/vehicles", svcs.vehicles)
        .use_service("/deliveries", svcs.deliveries)
        .use_service("/operations", svcs.operations)
        .use_service("/employees", svcs.employees)
        .use_service("/tomtom", svcs.tomtom)
        .use_service("/jobs", svcs.jobs)
        .use_service("/rules", svcs.rules)
        .use_service("/certifications", svcs.certifications)
        .service("/health", || async { "ok" });

    // Add CORS middleware to allow browser requests
    ax.router = ax.router
        .layer(tower_http::cors::CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any))
        .fallback_service(tower_http::services::ServeDir::new("static"));

    Ok(ax)
}
