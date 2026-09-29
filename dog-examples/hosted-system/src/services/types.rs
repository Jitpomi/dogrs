//! Application service types shared across services and background workers.

use std::sync::Arc;
use tokio_postgres::Client;

pub use super::payments::payments_schema::RecordPayment;

#[derive(Clone)]
pub struct BillingContext {
    pub db: Arc<Client>,
    pub tenant: String,
    pub crash_after_effect: bool,
}
