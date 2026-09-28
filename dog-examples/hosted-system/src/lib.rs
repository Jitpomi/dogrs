mod admission;
pub mod app;
mod capacity;
pub mod channels;
pub mod connections;
pub mod hooks;
mod recovery;
pub mod runner;
pub mod services;

use std::sync::Arc;

use anyhow::Result;
use dog_core::DogApp;
use dog_queue::{QueueAdapter, QueueBackend};
use dog_transport::{http::DogHttpService, HttpOptions, IntoDogService};
use serde_json::Value;

pub use app::build_app;
pub use services::{adapters::PaymentsAdapter, BillingContext, BillingService, RecordPayment};

/// Standard zero-argument construction API matching other DogRS applications.
/// Uses an in-memory queue adapter for testing without external services.
pub async fn build() -> Result<(DogApp<Value, ()>, DogHttpService<Value, ()>)> {
    let tenant =
        std::env::var("DOGRS_TEST_TENANT").unwrap_or_else(|_| "dogrs-test-default".into());
    let backend = dog_queue::backend::memory::MemoryBackend::new();
    let adapter = Arc::new(QueueAdapter::new(backend));
    adapter.register_job::<RecordPayment>().await?;
    build_with(adapter, tenant).await
}

/// Parameterized construction API with a custom queue adapter and tenant.
pub async fn build_with<B: QueueBackend + 'static>(
    adapter: Arc<QueueAdapter<B>>,
    tenant: String,
) -> Result<(DogApp<Value, ()>, DogHttpService<Value, ()>)> {
    let app = build_app(adapter, tenant)?;
    let service = app.clone().into_service(HttpOptions::new());
    Ok((app, service))
}
