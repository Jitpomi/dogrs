use crate::services::SocialParams;
use anyhow::Result;
use dog_core::DogAppBuilder;
use serde_json::Value;

pub async fn build_builder() -> Result<DogAppBuilder<Value, SocialParams>> {
    let mut builder: DogAppBuilder<Value, SocialParams> = DogAppBuilder::new();
    builder.set("http.host", "127.0.0.1");
    builder.set("http.port", "3036");
    crate::hooks::global_hooks(&mut builder)?;
    crate::channels::configure(&mut builder)?;
    crate::typedb::TypeDBState::setup_db(&mut builder).await?;
    Ok(builder)
}
