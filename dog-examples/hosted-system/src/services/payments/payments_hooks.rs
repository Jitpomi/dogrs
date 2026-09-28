//! Hooks specific to the payments service.

use anyhow::Result;
use dog_core::DogAppBuilder;
use serde_json::Value;

/// Register service-specific hooks for the payments service.
pub fn register_hooks(_app: &mut DogAppBuilder<Value, ()>) -> Result<()> {
    // The payments service authenticates with a dedicated bearer token at the HTTP transport layer
    // and validates invoices and execution modes via RecordPayment::validate before enqueueing.
    Ok(())
}
