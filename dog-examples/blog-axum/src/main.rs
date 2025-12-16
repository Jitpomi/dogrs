mod app;
mod hooks;
mod channels;
mod services;

use std::sync::Arc;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let ax = app::relay_app()?;
    let state = Arc::new(services::RelayState::default());

    hooks::global_hooks(ax.app.as_ref());
    channels::configure(ax.app.as_ref())?;

    let posts = services::configure(
        ax.app.as_ref(),
        Arc::clone(&state),
    )?;

    let ax = ax
        .use_service("/posts", posts)
        .service("/health", || async {"ok" });

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
