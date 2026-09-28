use dog_core::{DogApp, DogAppBuilder};
use dog_transport::{CliOptions, GrpcOptions, HttpOptions, IntoDogService};
use serde_json::Value;
pub fn build() -> DogApp<Value, crate::services::types::Params> {
    let mut builder = DogAppBuilder::new();
    crate::services::configure(&mut builder);
    builder.build()
}
pub async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = build();
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
        "iroh" => {
            let router = app
                .into_service(dog_transport::IrohOptions::new(b"dogrs/echo/1".to_vec()))
                .await?;
            eprintln!("Iroh endpoint: {:?}", router.endpoint().id());
            tokio::signal::ctrl_c().await?;
            router.shutdown().await?;
        }
        "cli" => app.into_service(CliOptions::new()).run_stdio().await?,
        _ => return Err("usage: transport-demo [http|grpc|cli|iroh]".into()),
    }
    Ok(())
}
