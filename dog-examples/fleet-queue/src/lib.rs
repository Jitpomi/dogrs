pub mod app;
pub mod background;
pub mod channels;
pub mod config;
pub mod hooks;
pub mod services;
pub mod typedb;

use serde_json::Value;
pub use services::FleetParams;
use std::sync::Arc;
use dog_core::DogApp;
use dog_transport::{HttpOptions, IntoDogService, http::DogHttpService};

pub async fn build() -> anyhow::Result<(DogApp<Value, FleetParams>, DogHttpService<Value, FleetParams>)> {
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

    // Configure the channels pub/sub system
    crate::channels::configure(&mut builder)?;

    // Store background system in app state for access within background jobs
    builder.set("background_system", Arc::clone(&background_system));

    // Build the app (moves the builder)
    let dog_app = builder.build();

    let http_service = dog_app.clone().into_service(
        HttpOptions::default()
            .tenant_header("x-tenant-id")
            .route("/vehicles", "vehicles")
            .route("/deliveries", "deliveries")
            .route("/operations", "operations")
            .route("/employees", "employees")
            .route("/tomtom", "tomtom")
            .route("/jobs", "jobs")
            .route("/rules", "rules")
            .route("/certifications", "certifications")
    );

    // Start background system with built app
    background_system.start(dog_app.clone()).await?;

    Ok((dog_app, http_service))
}
