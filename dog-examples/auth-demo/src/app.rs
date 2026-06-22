use anyhow::Result;
use dog_core::DogApp;
use dog_transport::{HttpOptions, IntoDogService, http::DogHttpService};
use serde_json::Value;

use crate::services::AuthDemoParams;

pub async fn auth_app() -> Result<(DogApp<Value, AuthDemoParams>, DogHttpService<Value, AuthDemoParams>)> {
    dotenvy::from_filename("dog-examples/auth-demo/.env").ok();
    dotenvy::dotenv().ok();

    let mut builder: dog_core::DogAppBuilder<Value, AuthDemoParams> =
        dog_core::DogAppBuilder::new();

    crate::config::config(&mut builder)?;
    let auth_adapter = crate::auth::strategies(&mut builder)?;
    let oauth_raw = crate::services::configure(&mut builder, auth_adapter.clone())?;
    crate::hooks::global_hooks(&mut builder);
    crate::channels::configure(&mut builder)?;
    
    let dog_app = builder.build();
    auth_adapter.setup(dog_app.clone());
    oauth_raw.setup(dog_app.clone());

    let http_service = dog_app.clone().into_service(
        HttpOptions::default()
            .tenant_header("x-tenant-id")
            .enable_cors(true)
            .route("/messages", "messages")
            .route("/users", "users")
            .route("/oauth", "oauth")
    );

    Ok((dog_app, http_service))
}
