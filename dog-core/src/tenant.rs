//! Core multi-tenant types for DogRS.

/// A simple tenant identifier.
/// Later this can be a UUID, slug, or composite key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TenantId(pub String);

/// Context carried with every DogRS operation.
///
/// This will be passed into services, hooks, and jobs so that
/// all logic is explicitly tenant-aware.
#[derive(Debug, Clone)]
pub struct TenantContext {
    pub tenant_id: TenantId,
    // TODO: add workspace_id, plan, feature flags, etc.
}

impl TenantContext {
    /// Convenience constructor from a string.
    pub fn new<S: Into<String>>(tenant: S) -> Self {
        Self {
            tenant_id: TenantId(tenant.into()),
        }
    }
}
