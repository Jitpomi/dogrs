mod app;
mod channels;
mod hooks;
mod services;

use std::sync::Arc;

use anyhow::Result;
use dog_core::DogApp;
use dog_transport::{HttpOptions, IntoDogService, http::DogHttpService};
use serde_json::Value;

pub use crate::services::BlogParams;

pub async fn build() -> Result<(DogApp<Value, BlogParams>, DogHttpService<Value, BlogParams>)> {
    let mut builder = app::build_builder().await?;
    let state = Arc::new(services::BlogState::default());

    services::configure(&mut builder, Arc::clone(&state))?;

    let dog = builder.build();

    let http_service = dog.clone().into_service(
        HttpOptions::default()
            .route("/posts", "posts")
            .route("/authors", "authors")
    );

    Ok((dog, http_service))
}
