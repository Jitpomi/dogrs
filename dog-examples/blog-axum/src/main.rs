use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let ax = blog_axum::build()?;

    let host = ax
        .app
        .get("http.host")
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let port = ax
        .app
        .get("http.port")
        .unwrap_or_else(|| "3030".to_string());

    let addr = format!("{host}:{port}");

    println!("[relay] listening on http://{addr}");

    ax.listen(addr).await?;

    Ok(())
}
