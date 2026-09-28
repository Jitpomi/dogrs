use crate::services::MusicParams;
use anyhow::Result;
use dog_core::DogAppBuilder;
use serde_json::Value;

pub async fn build_builder() -> Result<DogAppBuilder<Value, MusicParams>> {
    let mut builder: DogAppBuilder<Value, MusicParams> = DogAppBuilder::new();

    // Use environment variables with fallback defaults
    let host = std::env::var("HTTP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("HTTP_PORT").unwrap_or_else(|_| "3030".to_string());

    builder.set("http.host", host);
    builder.set("http.port", port);
    crate::hooks::global_hooks(&mut builder)?;
    crate::channels::configure(&mut builder)?;

    crate::rustfs::RustFsState::setup_store(&mut builder).await?;
    Ok(builder)
}

// Compose API and static routes in one place so integration tests exercise the real routing.
dog_transport::declare_adapter!(axum, to_endpoint, MusicParams);
pub fn http_router(
    dog: &dog_core::DogApp<Value, MusicParams>,
    service: dog_transport::http::DogHttpService<Value, MusicParams>,
    static_dir: impl AsRef<std::path::Path>,
) -> Result<axum::Router> {
    Ok(axum::Router::new()
        .merge(crate::uploads::router(dog)?)
        .route("/health", axum::routing::get(|| async { "ok" }))
        .fallback_service(
            tower_http::services::ServeDir::new(static_dir)
                .fallback(to_endpoint(service))
                .call_fallback_on_method_not_allowed(true),
        )
        .layer(crate::multipart::MultipartToJson::with_config(
            crate::multipart_config(),
        )))
}
