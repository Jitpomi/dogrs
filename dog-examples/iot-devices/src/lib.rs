mod app;
mod channels;
mod hooks;
mod services;

use anyhow::Result;
use dog_core::DogApp;
use dog_transport::{IntoDogService, HttpOptions, http::DogHttpService};
use serde_json::Value;

pub use services::DemoParams;

pub async fn build() -> Result<(DogApp<Value, DemoParams>, DogHttpService<Value, DemoParams>)> {
    let mut builder = app::build_builder().await?;
    
    services::configure(&mut builder)?;

    let dog = builder.build();

    // Create HTTP service via dog-transport options
    let http_service = dog.clone().into_service(
        HttpOptions::default()
            .tenant_header("x-tenant-id")
            .enable_cors(true)
            .route("/devices", "devices")
    );

    Ok((dog, http_service))
}
