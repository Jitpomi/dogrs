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
pub use runner::{dispatch_role, env, run, run_app, tenant, LEASE};
pub use services::{adapters::PaymentsAdapter, BillingContext, BillingService, RecordPayment};

/// Reusable construction API exposing the DogApp and DogHttpService for tests and other entry points.
pub async fn build<B: QueueBackend + 'static>(
    adapter: Arc<QueueAdapter<B>>,
    tenant: String,
) -> Result<(DogApp<Value, ()>, DogHttpService<Value, ()>)> {
    let app = build_app(adapter, tenant)?;
    let service = app.clone().into_service(HttpOptions::new());
    Ok((app, service))
}
