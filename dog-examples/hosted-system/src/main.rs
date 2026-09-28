#[tokio::main]
async fn main() -> anyhow::Result<()> {
    hosted_system::run_app().await
}
