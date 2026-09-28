//! Application channel configuration and event routing.

use anyhow::Result;
use dog_core::DogAppBuilder;
use serde_json::Value;

/// Configure application event channels and routing.
pub fn configure(_builder: &mut DogAppBuilder<Value, ()>) -> Result<()> {
    // This queue acceptance harness does not expose event channels.
    Ok(())
}
