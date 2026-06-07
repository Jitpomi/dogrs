use anyhow::Result;

use serde_json::Value;
use crate::services::AuthDemoParams;

pub fn configure(_builder: &mut dog_core::DogAppBuilder<Value, AuthDemoParams>) -> Result<()> {
    // TODO: Add channel configuration for authentication demo
    Ok(())
}
