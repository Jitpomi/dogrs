use anyhow::Result;
use axum::{routing::get, Router};

dog_transport::declare_adapter!(axum, to_endpoint, social_typedb::SocialParams);

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

    let (dog, http_service) = social_typedb::build().await?;

    let host = dog
        .get("http.host")
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let port = dog
        .get("http.port")
        .unwrap_or_else(|| "3030".to_string());

    let addr = format!("{host}:{port}");

    println!("[social-typedb] listening on http://{addr}");

    let router = Router::new()
        .route("/health", get(|| async { "ok" }))
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .fallback_service(
            tower_http::services::ServeDir::new("dog-examples/social-typedb/static")
                .fallback(to_endpoint(http_service))
        );

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
