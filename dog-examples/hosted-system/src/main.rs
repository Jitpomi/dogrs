use anyhow::{Context, Result};
use hosted_system::{admission, connections, runner};

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

    match role.as_str() {
        "admission-native" => admission::native().await,
        "network-probe" => connections::network_probe().await,
        "init" => runner::init_schema().await,
        "inspect" => runner::inspect_schema().await,
        role if role == "capacity-local" || role.starts_with("recovery-") => {
            connections::dispatch_local(role).await
        }
        role => connections::dispatch(role).await,
    }
}
