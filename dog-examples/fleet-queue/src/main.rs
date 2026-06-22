use anyhow::Result;

dog_transport::declare_adapter!(axum, to_endpoint, fleet_queue::FleetParams);
dog_transport::declare_sse_adapter!(axum, to_sse, fleet_queue::channels::telemetry_channel());

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file
    if std::path::Path::new("dog-examples/fleet-queue/.env").exists() {
        dotenvy::from_path("dog-examples/fleet-queue/.env").ok();
    } else {
        dotenvy::dotenv().ok();
    }

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    let (dog, http_service) = fleet_queue::build().await?;

    let host = dog
        .get("http.host")
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let port = dog
        .get("http.port")
        .unwrap_or_else(|| "3030".to_string());

    let addr = format!("{host}:{port}");

    println!("[fleet-queue] listening on http://{addr}");

    let http_fallback = to_endpoint(http_service);
    let static_dir = if std::path::Path::new("dog-examples/fleet-queue/static").exists() {
        "dog-examples/fleet-queue/static"
    } else {
        "static"
    };
    let static_service = tower_http::services::ServeDir::new(static_dir)
        .fallback(http_fallback.clone());

    let router = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route("/config", axum::routing::get(|| async {
            // TomTom map API keys are intentionally served to the browser.
            let key = std::env::var("TOMTOM_API_KEY").unwrap_or_default();
            format!("{{\"tomtomApiKey\":\"{}\"}}", key)
        }))
        .route("/events", axum::routing::get(to_sse))
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .fallback_service(dog_transport::tower::service_fn(move |req: axum::http::Request<axum::body::Body>| {
            let mut static_svc = static_service.clone();
            let mut http_svc = http_fallback.clone();
            async move {
                use dog_transport::tower::Service;
                use axum::response::IntoResponse;
                if req.method() == axum::http::Method::GET || req.method() == axum::http::Method::HEAD {
                    static_svc.call(req).await.map(|res| res.into_response())
                } else {
                    http_svc.call(req).await.map(|res| res.into_response())
                }
            }
        }));

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
