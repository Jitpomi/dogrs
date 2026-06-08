use anyhow::Result;

use crate::services::AuthDemoParams;
use serde_json::Value;

pub fn configure(_builder: &mut dog_core::DogAppBuilder<Value, AuthDemoParams>) -> Result<()> {
    // TODO: Add channel configuration for authentication demo
    Ok(())
}
