use anyhow::Result;
use axum::{routing::get, Router};

dog_transport::declare_adapter!(axum, to_endpoint, iot_devices::DemoParams);
dog_transport::declare_ws_adapter!(axum, to_ws, iot_devices::DemoParams);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    let (dog, http_service) = iot_devices::build().await?;

    let host = dog
        .get("http.host")
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let port = dog
        .get("http.port")
        .unwrap_or_else(|| "3000".to_string());

    let addr = format!("{host}:{port}");

    tracing::info!("Listening on http://{addr}");
    tracing::info!("  Dashboard UI: http://{addr}");
    tracing::info!("  REST API:     GET/POST http://{addr}/api/devices");
    tracing::info!("  WebSocket:    ws://{addr}/ws");

    let extra_router = Router::new()
        .route("/ws", get(to_ws))
        .with_state(dog.clone());

    let router = Router::new()
        .nest_service("/api", to_endpoint(http_service))
        .merge(extra_router)
        .fallback_service(
            tower_http::services::ServeDir::new("dog-examples/iot-devices/static")
        );

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}

