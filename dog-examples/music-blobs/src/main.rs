use anyhow::Result;

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

    if std::env::args().nth(1).as_deref() == Some("recover") {
        music_blobs::uploads::recover(&dog).await?;
        return Ok(());
    }
    let host = dog
        .get("http.host")
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let port = dog.get("http.port").unwrap_or_else(|| "3030".to_string());

    let addr = format!("{host}:{port}");

    println!("[music-blobs] listening on http://{addr}");

    let static_dir = std::env::var("STATIC_DIR")
        .unwrap_or_else(|_| format!("{}/static", env!("CARGO_MANIFEST_DIR")));

    anyhow::ensure!(
        host.parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
            || host == "localhost"
            || std::env::var("MUSIC_ALLOW_CONTAINER_BIND").as_deref() == Ok("1"),
        "music-blobs is a public single-tenant demo; bind it to loopback"
    );
    let router = music_blobs::http_router(&dog, http_service, static_dir)?;

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    Ok(())
}
