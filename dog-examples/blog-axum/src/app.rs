use crate::services::BlogParams;
use anyhow::Result;
use dog_core::DogAppBuilder;
use serde_json::Value;

pub async fn build_builder() -> Result<DogAppBuilder<Value, BlogParams>> {
    let mut builder: DogAppBuilder<Value, BlogParams> = DogAppBuilder::new();

    builder.set("http.host", "127.0.0.1");
    builder.set("http.port", "3036");

    crate::hooks::register_global_hooks(&mut builder)?;
    crate::channels::configure(&mut builder)?;

    Ok(builder)
}
