//! # Calendar Server Entry Point (`main.rs`)
//!
//! Executable entry point responsible for:
//! - Initializing environment variables (`dotenvy`) and tracing instrumentation.
//! - Bootstrapping the application via [`calendar::build`].
//! - Composing Axum HTTP endpoints, the `/ws` WebSocket route, and static UI file serving.
//! - Binding the TCP listener and managing graceful shutdown.

use anyhow::Result;
use axum::{routing::get, Router};

dog_transport::declare_adapter!(axum, to_endpoint, calendar::CalendarParams);
dog_transport::declare_ws_adapter!(axum, to_ws, calendar::CalendarParams);

#[tokio::main]
async fn main() -> Result<()> {
    if std::path::Path::new("dog-examples/calendar/.env").exists() {
        dotenvy::from_path("dog-examples/calendar/.env").ok();
    } else {
        dotenvy::dotenv().ok();
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let (dog, http_service) = calendar::build().await?;

    let host = dog
        .get("http.host")
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let port = dog.get("http.port").unwrap_or_else(|| "3036".to_string());

    let addr = format!("{host}:{port}");

    tracing::info!("Listening on http://{addr}");
    tracing::info!("  Calendar UI:  http://{addr}");
    tracing::info!("  REST API:     POST/GET http://{addr}/api/events");
    tracing::info!("  WebSocket:    ws://{addr}/ws");

    let extra_router = Router::new()
        .route("/ws", get(to_ws))
        .with_state(dog.clone());

    let router = Router::new()
        .nest_service("/api", to_endpoint(http_service))
        .merge(extra_router)
        .fallback_service(tower_http::services::ServeDir::new(
            "dog-examples/calendar/static",
        ));

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    Ok(())
}
