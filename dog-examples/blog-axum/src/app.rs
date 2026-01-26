// TODO: implement
use anyhow::Result;
use dog_axum::{axum, AxumApp};
use dog_core::DogApp;
use serde_json::Value;
use crate::services::BlogParams;

pub async fn blog_app() -> Result<AxumApp<Value, BlogParams>> {
    let dog_app: DogApp<Value, BlogParams> = DogApp::new();
    dog_app.set("http.host", "127.0.0.1");
    dog_app.set("http.port", "3036");
    crate::hooks::global_hooks(&dog_app);
    crate::channels::configure(&dog_app)?;
    let blog_app: AxumApp<Value, BlogParams> = axum(dog_app);
    Ok(blog_app)
}
