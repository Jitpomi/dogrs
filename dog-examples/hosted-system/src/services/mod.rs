//! Application services module declaration and registration.

use std::sync::Arc;

use anyhow::Result;
use dog_core::DogAppBuilder;
use dog_queue::{QueueAdapter, QueueBackend};
use serde_json::Value;

pub mod payments;
pub mod types;

pub use payments::{BillingService, RecordPayment};
pub use types::*;

/// Configure and register all services with the DogAppBuilder.
pub fn configure<B: QueueBackend + 'static>(
    app: &mut DogAppBuilder<Value, ()>,
    adapter: Arc<QueueAdapter<B>>,
    tenant: String,
) -> Result<()> {
    let service = Arc::new(BillingService::new(adapter, tenant));
    app.register_service("payments", service);
    payments::payments_shared::register_hooks(app)?;
    Ok(())
}
