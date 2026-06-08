mod app;
mod channels;
mod hooks;
mod services;

use std::sync::Arc;

use anyhow::Result;
use dog_axum::AxumApp;
use serde_json::Value;

use crate::services::BlogParams;

pub async fn build() -> Result<AxumApp<Value, BlogParams>> {
    let mut builder = app::build_builder().await?;
    let state = Arc::new(services::BlogState::default());

    let svcs = services::configure(&mut builder, Arc::clone(&state))?;

    let ax = dog_axum::axum(builder.build())
        .use_service("/posts", svcs.posts)
        .use_service("/authors", svcs.authors)
        .service("/health", || async { "ok" });

    Ok(ax)
}
