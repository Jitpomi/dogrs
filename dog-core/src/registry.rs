use std::collections::HashMap;
use std::sync::Arc;
use anyhow::{anyhow, Result};
use crate::{DogService, TenantContext};

/// A simple registry that maps service names to DogService instances.
///
/// This is the core of DogRS: named services that can be called
/// from any transport (HTTP, CLI, jobs, P2P, etc.).
pub struct DogServiceRegistry<R, P = ()> {
    services: HashMap<String, Arc<dyn DogService<R, P>>>,
}

impl<R, P> DogServiceRegistry<R, P> {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
        }
    }

    /// Register a service under a given name.
    pub fn register<S>(&mut self, name: S, service: Arc<dyn DogService<R, P>>)
    where
        S: Into<String>,
    {
        self.services.insert(name.into(), service);
    }

    /// Look up a service by name.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn DogService<R, P>>> {
        self.services.get(name)
    }
}

impl<R, P> Default for DogServiceRegistry<R, P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P: Clone> DogServiceRegistry<R, P> {
    /// Call `list` on a named service.
    pub async fn list(
        &self,
        name: &str,
        ctx: &TenantContext,
        params: P,
    ) -> Result<Vec<R>> {
        let service = self
            .get(name)
            .ok_or_else(|| anyhow!("DogService not found: {name}"))?;

        service.list(ctx, params).await
    }

    /// Call `create` on a named service.
    pub async fn create(
        &self,
        name: &str,
        ctx: &TenantContext,
        params: P,
        record: R,
    ) -> Result<R> {
        let service = self
            .get(name)
            .ok_or_else(|| anyhow!("DogService not found: {name}"))?;

        service.create(ctx, params, record).await
    }
}
