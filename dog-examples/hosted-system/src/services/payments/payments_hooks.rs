//! Hooks specific to the payments service.

use std::sync::Arc;

use anyhow::{ensure, Result};
use async_trait::async_trait;
use dog_core::hooks::{DogBeforeHook, HookContext};
use dog_core::DogAppBuilder;
use serde_json::Value;

/// BeforeHook that validates payment job submissions and batch snapshot lookups.
pub struct ValidatePaymentRequest;

#[async_trait]
impl DogBeforeHook<Value, ()> for ValidatePaymentRequest {
    async fn run(&self, ctx: &mut HookContext<Value, ()>) -> Result<()> {
        if let Some(data) = ctx.data.as_ref() {
            if let Some(ids) = data.get("status_ids") {
                let ids: Vec<String> = serde_json::from_value(ids.clone())?;
                ensure!(
                    ids.len() <= 1000 && ids.iter().all(|id| !id.is_empty()),
                    "invalid status batch"
                );
            } else if let Some(invoice) = data.get("invoice").and_then(|v| v.as_str()) {
                ensure!(
                    !invoice.is_empty() && invoice.len() <= 100,
                    "invalid synthetic invoice"
                );
                if let Some(mode) = data.get("mode").and_then(|v| v.as_str()) {
                    ensure!(
                        ["normal", "retry", "long", "crash", "permanent"].contains(&mode),
                        "invalid mode"
                    );
                }
            }
        }
        Ok(())
    }
}

/// Register service-specific hooks for the payments service.
pub fn register_hooks(app: &mut DogAppBuilder<Value, ()>) -> Result<()> {
    app.service_hooks("payments", |h| {
        h.before_create(Arc::new(ValidatePaymentRequest));
    });
    Ok(())
}
