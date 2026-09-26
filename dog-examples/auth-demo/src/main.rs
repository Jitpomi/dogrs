use anyhow::Result;
use actix_web::{web, App, HttpServer, HttpResponse};
use std::sync::Arc;

dog_transport::declare_adapter!(actix, to_endpoint, auth_demo::AuthDemoParams);

#[actix_web::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let (dog, http_service) = auth_demo::build().await?;

    let host = dog
        .get("http.host")
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let port = dog
        .get("http.port")
        .unwrap_or_else(|| "3030".to_string());

    let addr = format!("{host}:{port}");

    println!("[auth-demo] listening on http://{addr}");

    let shared_dog = Arc::new(dog);
    let shared_http_service = web::Data::new(http_service);

    HttpServer::new(move || {
        let app_clone = Arc::clone(&shared_dog);
        App::new()
            .app_data(shared_http_service.clone())
            .route("/health", web::get().to(|| async { HttpResponse::Ok().body("ok") }))
            .configure(|cfg| auth_demo::configure_oauth(cfg, app_clone))
            .default_service(web::to(to_endpoint))
    })
    .bind(&addr)?
    .run()
    .await?;

    Ok(())
}
