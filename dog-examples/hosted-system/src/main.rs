use anyhow::{Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    tracing_subscriber::fmt()
        .with_env_filter("warn")
        .with_writer(std::io::stderr)
        .init();

    let role = std::env::args()
        .nth(1)
        .context("usage: hosted-system init|inspect|serve|worker")?;

    hosted_system::runner::dispatch_role(&role).await
}
