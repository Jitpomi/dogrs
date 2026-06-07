use anyhow::Result;
use dog_axum::{axum, AxumApp};

use serde_json::Value;
use crate::services::AuthDemoParams;


pub async fn auth_app() -> Result<AxumApp<Value, AuthDemoParams>> {
    dotenvy::from_filename("dog-examples/auth-demo/.env").ok();
    dotenvy::dotenv().ok();

    let mut builder: dog_core::DogAppBuilder<Value, AuthDemoParams> = dog_core::DogAppBuilder::new();

    crate::config::config(&mut builder)?;
    let auth_adapter = crate::auth::strategies(&mut builder)?;
    let svcs = crate::services::configure(&mut builder, auth_adapter.auth().clone())?;
    crate::hooks::global_hooks(&mut builder);
    crate::channels::configure(&mut builder)?;
    let dog_app = builder.build();
    auth_adapter.setup(dog_app.clone());
    svcs.oauth_raw.setup(dog_app.clone());

    let mut ax: AxumApp<Value, AuthDemoParams> = axum(dog_app);
    
    ax = ax
        .use_service("/messages", svcs.messages)
        .use_service("/users", svcs.users)
        .use_service("/auth", svcs.auth_svc)
        .use_service("/oauth", svcs.oauth);

    let ax = crate::auth::oauth2::google::http::mount(ax);
    Ok(ax)
}
