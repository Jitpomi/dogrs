use anyhow::{Context, Result};
use hosted_system::{admission, connections, runner};

fn main() -> Result<()> {
    let role = std::env::args()
        .nth(1)
        .context("usage: hosted-system init|inspect|serve|worker")?;
    let mut runtime = tokio::runtime::Builder::new_multi_thread();
    runtime.enable_all();
    if std::env::var("DOGRS_CAPACITY_FAIR_SCHEDULER").as_deref() == Ok("1") {
        anyhow::ensure!(
            role == "capacity-local",
            "fair scheduling is a capacity experiment"
        );
        // Cross-runtime producers submit into the global queue. Check it on
        // every scheduling tick rather than allowing ready local worker tasks
        // to postpone admission under a continuously busy consumer workload.
        runtime.global_queue_interval(1);
    }
    runtime.build()?.block_on(run(role))
}

async fn run(role: String) -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    tracing_subscriber::fmt()
        .with_env_filter("warn")
        .with_writer(std::io::stderr)
        .init();

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
