use anyhow::Result;
use poem::{get, Route};

dog_transport::declare_adapter!(poem, to_endpoint, blog::BlogParams);

#[poem::handler]
fn health_handler() -> &'static str {
    "ok"
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let (dog, http_service) = blog::build().await?;

    let host = dog
        .get("http.host")
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let port = dog
        .get("http.port")
        .unwrap_or_else(|| "3030".to_string());

    let addr = format!("{host}:{port}");

    println!("[relay] listening on http://{addr}");

    let router = Route::new()
        .at("/health", get(health_handler))
        .nest("/", to_endpoint(http_service));

    let listener = poem::listener::TcpListener::bind(&addr);
    poem::Server::new(listener).run(std::sync::Arc::new(router)).await?;

    Ok(())
}
