pub mod app;
pub mod channels;
pub mod hooks;
pub mod services;
pub mod typedb;

use dog_core::DogApp;
use dog_transport::{IntoDogService, http::DogHttpService};
use serde_json::Value;
pub use services::SocialParams;
use std::sync::Arc;

pub async fn build() -> anyhow::Result<(DogApp<Value, SocialParams>, DogHttpService<Value, SocialParams>)> {
    let mut builder = app::build_builder().await?;

    let state = builder
        .get::<Arc<typedb::TypeDBState>>("typedb")
        .ok_or(anyhow::anyhow!("TypeDBState not found"))?;

    services::configure(&mut builder, Arc::clone(&state))?;

    let dog = builder.build();

    let http_service = dog.clone().into_service(
        dog_transport::HttpOptions::default()
            .route("/persons", "persons")
            .route("/organizations", "organizations")
            .route("/groups", "groups")
            .route("/posts", "posts")
            .route("/comments", "comments")
    );

    Ok((dog, http_service))
}
