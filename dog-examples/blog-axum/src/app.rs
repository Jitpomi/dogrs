// TODO: implement
use anyhow::Result;
use dog_axum::{axum, AxumApp};
use dog_core::DogApp;
use serde_json::Value;
use crate::services::RelayParams;

pub fn relay_app() -> Result<AxumApp<Value, RelayParams>> {
    let dog_app: DogApp<Value, RelayParams> = DogApp::new();
    dog_app.set("http.host", "127.0.0.1");
    dog_app.set("http.port", "3036");
    let relay_app: AxumApp<Value, RelayParams> = axum(dog_app);
    Ok(relay_app)
}
