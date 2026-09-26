//! Public, stateless echo demo. Add authentication hooks for private services.
use dog_core::{DogAppBuilder, DogService, TenantContext};
use dog_transport::{CliOptions, GrpcOptions, HttpOptions, IntoDogService};
use serde_json::Value;
use std::sync::Arc;
struct Echo;
#[async_trait::async_trait]
impl DogService<Value, ()> for Echo {
    async fn create(&self, _: &TenantContext, data: Value, _: ()) -> anyhow::Result<Value> {
        Ok(data)
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut builder = DogAppBuilder::<Value, ()>::new();
    builder.register_service("echo", Arc::new(Echo));
    let app = builder.build();
    match std::env::args().nth(1).as_deref().unwrap_or("http") {
        "http" => {
            let service = app.into_service(HttpOptions::new());
            let router = axum::Router::new().fallback_service(service);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
            eprintln!("HTTP: http://127.0.0.1:3000/echo");
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
        }
        "grpc" => {
            eprintln!("gRPC: 127.0.0.1:50051 (reflection enabled)");
            app.into_service(GrpcOptions::new().enable_reflection(true))
                .serve_with_shutdown("127.0.0.1:50051".parse()?, async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
        }
        "cli" => app.into_service(CliOptions::new()).run_stdio().await?,
        _ => return Err("usage: transport-demo [http|grpc|cli]".into()),
    }
    Ok(())
}
