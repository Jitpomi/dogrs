use crate::services::MusicParams;
use anyhow::Result;
use dog_core::DogAppBuilder;
use serde_json::Value;

pub async fn build_builder() -> Result<DogAppBuilder<Value, MusicParams>> {
    let mut builder: DogAppBuilder<Value, MusicParams> = DogAppBuilder::new();

    // Use environment variables with fallback defaults
    let host = std::env::var("HTTP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("HTTP_PORT").unwrap_or_else(|_| "3030".to_string());

    builder.set("http.host", host);
    builder.set("http.port", port);
    crate::hooks::global_hooks(&mut builder)?;
    crate::channels::configure(&mut builder)?;

    crate::rustfs::RustFsState::setup_store(&mut builder).await?;
    Ok(builder)
}
