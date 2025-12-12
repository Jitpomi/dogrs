use async_trait::async_trait;
use anyhow::{anyhow, Result};

use crate::tenant::TenantContext;

/// Standard service methods, similar to Feathers:
/// find, get, create, update, patch, remove.
///
/// Custom methods are declared via `Custom("methodName")`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ServiceMethodKind {
    Find,
    Get,
    Create,
    Update,
    Patch,
    Remove,
    Custom(&'static str),
}

/// Capabilities describe which methods a service wants to expose
/// to the outside world (HTTP, WebSockets, P2P, etc.).
///
/// Adapters (like dog-axum) can use this to mount only allowed routes.
#[derive(Debug, Clone)]
pub struct ServiceCapabilities {
    pub allowed_methods: Vec<ServiceMethodKind>,
}

impl ServiceCapabilities {
    /// Full CRUD, equivalent to Feathers default:
    /// ['find', 'get', 'create', 'patch', 'update', 'remove']
    pub fn standard_crud() -> Self {
        use ServiceMethodKind::*;
        Self {
            allowed_methods: vec![Find, Get, Create, Update, Patch, Remove],
        }
    }

    /// Minimal example: only `find` and `create`.
    pub fn minimal() -> Self {
        use ServiceMethodKind::*;
        Self {
            allowed_methods: vec![Find, Create],
        }
    }

    /// Helper for building from a list.
    pub fn from_methods(methods: Vec<ServiceMethodKind>) -> Self {
        Self {
            allowed_methods: methods,
        }
    }
}

/// Core DogRS service trait, inspired by FeathersJS:
///
/// - `find`   → list/query many
/// - `get`    → fetch one by id
/// - `create` → create one (or conceptually many)
/// - `update` → full replace
/// - `patch`  → partial update
/// - `remove` → delete one or many
///
/// All methods have default implementations that return
/// "Method not implemented", so a service can override only
/// what it actually supports.
#[async_trait]
#[async_trait]
pub trait DogService<R, P = ()>: Send + Sync
where
    R: Send + 'static,
    P: Send + 'static,
{

    /// Describe which methods this service wants to expose.
    ///
    /// Adapters (HTTP, P2P, etc.) should respect this when deciding
    /// what is callable from the outside world.
    fn capabilities(&self) -> ServiceCapabilities {
        // By default, assume full CRUD.
        ServiceCapabilities::standard_crud()
    }

    /// Find many records (optionally filtered by params).
    async fn find(&self, _ctx: &TenantContext, _params: P) -> Result<Vec<R>> {
        Err(anyhow!("Method not implemented: find"))
    }

    /// Get a single record by id.
    async fn get(&self, _ctx: &TenantContext, _id: &str, _params: P) -> Result<R> {
        Err(anyhow!("Method not implemented: get"))
    }

    /// Create a new record.
    ///
    /// For many-record semantics, an adapter or higher-level
    /// service can wrap this in a loop or accept Vec<R>.
    async fn create(&self, _ctx: &TenantContext, _data: R, _params: P) -> Result<R> {
        Err(anyhow!("Method not implemented: create"))
    }

    /// Fully replace an existing record.
    ///
    /// `id` is required (no multi-update here at core level).
    async fn update(
        &self,
        _ctx: &TenantContext,
        _id: &str,
        _data: R,
        _params: P,
    ) -> Result<R> {
        Err(anyhow!("Method not implemented: update"))
    }

    /// Partially update an existing record.
    ///
    /// `id` can be `None` to indicate "multi" semantics if
    /// an adapter / implementation supports it.
    async fn patch(
        &self,
        _ctx: &TenantContext,
        _id: Option<&str>,
        _data: R,
        _params: P,
    ) -> Result<R> {
        Err(anyhow!("Method not implemented: patch"))
    }

    /// Remove an existing record.
    ///
    /// `id` can be `None` to indicate "multi" semantics if
    /// an adapter / implementation supports it.
    async fn remove(
        &self,
        _ctx: &TenantContext,
        _id: Option<&str>,
        _params: P,
    ) -> Result<R> {
        Err(anyhow!("Method not implemented: remove"))
    }
}
