use crate::services::MusicParams;
use anyhow::Result;
use dog_axum::{axum, AxumApp};
use dog_core::DogApp;
use serde_json::Value;

pub async fn music_app() -> Result<AxumApp<Value, MusicParams>> {
    let dog_app: DogApp<Value, MusicParams> = DogApp::new();

    // Use environment variables with fallback defaults
    let host = std::env::var("HTTP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("HTTP_PORT").unwrap_or_else(|_| "3030".to_string());

    dog_app.set("http.host", host);
    dog_app.set("http.port", port);
    crate::hooks::global_hooks(&dog_app);
    crate::channels::configure(&dog_app)?;

    let music_app: AxumApp<Value, MusicParams> = axum(dog_app);
    crate::rustfs::RustFsState::setup_store(music_app.app.as_ref()).await?;
    Ok(music_app)
}
