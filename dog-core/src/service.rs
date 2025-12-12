use async_trait::async_trait;
use anyhow::Result;

use crate::tenant::TenantContext;

/// Core service trait in DogRS.
///
/// R = record type (e.g. HelloRecord, Audience, Campaign)
/// P = params type (filters, query options, etc.)
#[async_trait]
pub trait DogService<R, P = ()>: Send + Sync {
    /// List records for a given tenant and params.
    async fn list(&self, ctx: &TenantContext, params: P) -> Result<Vec<R>>;

    /// Create a new record for a given tenant.
    async fn create(&self, ctx: &TenantContext, params: P, record: R) -> Result<R>;
}
