use anyhow::Result;
use dog_axum::{axum, AxumApp};
use dog_core::DogApp;
use serde_json::Value;
use crate::services::AuthDemoParams;


pub fn auth_app() -> Result<AxumApp<Value, AuthDemoParams>> {
    let dog_app: DogApp<Value, AuthDemoParams> = DogApp::new();

    dog_app.set("http.host", "127.0.0.1");
    dog_app.set("http.port", "3000");

    let ax: AxumApp<Value, AuthDemoParams> = axum(dog_app);
    Ok(ax)
}

