use anyhow::Result;
use dog_axum::{axum, AxumApp};
use dog_core::DogApp;
use serde_json::Value;
use crate::services::FleetParams;
use crate::config;

pub async fn fleet_app() -> Result<AxumApp<Value, FleetParams>> {
    let dog_app: DogApp<Value, FleetParams> = DogApp::new();
    
    // Apply all configuration
    config::config(&dog_app)?;

    crate::hooks::global_hooks(&dog_app);
    crate::channels::configure(&dog_app)?;
    
    let fleet_app: AxumApp<Value, FleetParams> = axum(dog_app);
    crate::typedb::TypeDBState::setup_db(fleet_app.app.as_ref()).await?;
    Ok(fleet_app)
}