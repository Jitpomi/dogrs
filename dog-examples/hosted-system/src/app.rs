//! Application construction and composition of services, global hooks, and channels.

use std::sync::Arc;

use anyhow::Result;
use dog_core::{DogApp, DogAppBuilder};
use dog_queue::{QueueAdapter, QueueBackend};
use serde_json::Value;

/// Constructs the DogApp by composing services, global hooks, and channels.
pub fn build_app<B: QueueBackend + 'static>(
    adapter: Arc<QueueAdapter<B>>,
    tenant: String,
) -> Result<DogApp<Value, ()>> {
    let mut builder = DogAppBuilder::<Value, ()>::new();
    crate::hooks::register_global_hooks(&mut builder)?;
    crate::channels::configure(&mut builder)?;
    crate::services::configure(&mut builder, adapter, tenant)?;
    Ok(builder.build())
}
