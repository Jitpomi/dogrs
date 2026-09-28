use crate::services::DemoParams;
use anyhow::Result;
use dog_core::DogAppBuilder;
use serde_json::Value;

pub async fn build_builder() -> Result<DogAppBuilder<Value, DemoParams>> {
    let mut builder: DogAppBuilder<Value, DemoParams> = DogAppBuilder::new();

    builder.set("http.host", "127.0.0.1");
    builder.set("http.port", "3000");
    // All simulated devices are public in this loopback-only example.
    builder.set("ws.public_broadcasts", "true");
    builder.set(
        "ws.allowed_origins",
        std::sync::Arc::new(vec![
            "http://127.0.0.1:3000".to_string(),
            "http://localhost:3000".to_string(),
        ]),
    );

    crate::hooks::register_global_hooks(&mut builder)?;
    crate::channels::configure(&mut builder)?;

    Ok(builder)
}
