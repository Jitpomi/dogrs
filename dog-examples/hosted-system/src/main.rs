#[tokio::main]
async fn main() -> anyhow::Result<()> {
    hosted_system::app::run_app().await
}
