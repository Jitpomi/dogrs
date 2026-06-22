use anyhow::Result;
use axum::{routing::get, Router};
use music_blobs::multipart::MultipartToJson;

dog_transport::declare_adapter!(axum, to_endpoint, music_blobs::MusicParams);

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    // Set RUST_LOG if not already set, but don't initialize tracing
    // Let the framework handle logging initialization
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let (dog, http_service) = music_blobs::build().await?;

    let host = dog
        .get("http.host")
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let port = dog
        .get("http.port")
        .unwrap_or_else(|| "3030".to_string());

    let addr = format!("{host}:{port}");

    println!("[music-blobs] listening on http://{addr}");

    let config = music_blobs::multipart_config();
    let static_dir = std::env::var("STATIC_DIR")
        .unwrap_or_else(|_| format!("{}/static", env!("CARGO_MANIFEST_DIR")));

    let router = Router::new()
        .route("/health", get(|| async { "ok" }))
        .layer(MultipartToJson::with_config(config))
        .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024)) // 100MB to match dog-blob config
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .fallback_service(
            tower_http::services::ServeDir::new(static_dir)
                .fallback(to_endpoint(http_service))
        );

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
