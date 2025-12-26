use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Set RUST_LOG if not already set, but don't initialize tracing
    // Let the framework handle logging initialization
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    let ax = social_typedb::build().await?;

    let host = ax
        .app
        .get("http.host")
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let port = ax
        .app
        .get("http.port")
        .unwrap_or_else(|| "3030".to_string());

    let addr = format!("{host}:{port}");

    println!("[social-typedb] listening on http://{addr}");

    ax.listen(addr).await?;

    Ok(())
}