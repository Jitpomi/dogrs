use anyhow::Result;
use dog_core::DogApp;
use serde_json::Value;
use crate::services::AuthDemoParams;

pub fn configure(_app: &DogApp<Value, AuthDemoParams>) -> Result<()> {
    // TODO: Add channel configuration for authentication demo
    Ok(())
}
