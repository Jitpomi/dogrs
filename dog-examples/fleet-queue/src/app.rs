use crate::config;
use crate::services::FleetParams;
use anyhow::Result;
use serde_json::Value;

pub async fn build_builder() -> Result<dog_core::DogAppBuilder<Value, FleetParams>> {
    let mut builder: dog_core::DogAppBuilder<Value, FleetParams> = dog_core::DogAppBuilder::new();

    config::config(&mut builder)?;
    crate::hooks::global_hooks(&mut builder)?;
    crate::typedb::TypeDBState::setup_db(&mut builder).await?;
    Ok(builder)
}
