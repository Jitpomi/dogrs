use crate::services::DemoParams;
use anyhow::Result;
use dog_core::DogAppBuilder;
use serde_json::Value;

pub async fn build_builder() -> Result<DogAppBuilder<Value, DemoParams>> {
    let mut builder: DogAppBuilder<Value, DemoParams> = DogAppBuilder::new();

    builder.set("http.host", "127.0.0.1");
    builder.set("http.port", "3000");

    crate::hooks::register_global_hooks(&mut builder)?;
    crate::channels::configure(&mut builder)?;

    Ok(builder)
}
