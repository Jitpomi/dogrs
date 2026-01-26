use anyhow::Result;
use dog_axum::{axum, AxumApp};
use dog_core::DogApp;
use serde_json::Value;
use crate::services::AuthDemoParams;


pub fn auth_app() -> Result<AxumApp<Value, AuthDemoParams>> {
    dotenvy::from_filename("dog-examples/auth-demo/.env").ok();
    dotenvy::dotenv().ok();

    let dog_app: DogApp<Value, AuthDemoParams> = DogApp::new();

    crate::config::config(&dog_app)?;

    crate::auth::strategies(&dog_app)?;

    let ax: AxumApp<Value, AuthDemoParams> = axum(dog_app);
    Ok(ax)
}
