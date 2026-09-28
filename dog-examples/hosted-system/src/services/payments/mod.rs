//! Payments service module declaration and public API.

pub mod payments_hooks;
pub mod payments_schema;
pub mod payments_service;
pub mod payments_shared;

pub use payments_schema::{PaymentPayload, RecordPayment};
pub use payments_service::BillingService;
