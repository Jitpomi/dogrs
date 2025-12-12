use std::sync::Arc;
use anyhow::Result;
use crate::{
    DogConfig, DogHook, DogService, DogServiceRegistry,
};

/// DogApp is the central application container for DogRS.
///
/// It is framework-agnostic and does NOT know anything about:
/// - HTTP
/// - storage
/// - databases
/// - runtimes
/// - job schedulers
/// - P2P transports
///
/// Its only job is to hold:
/// - a registry of services
/// - a list of global hooks
///
/// Later, it will orchestrate the method pipeline (before/after/around/error).
pub struct DogApp<R, P = ()>
where
    R: Send + 'static,
    P: Send + 'static,
{
    pub(crate) registry: DogServiceRegistry<R, P>,
    pub(crate) global_hooks: Vec<Arc<dyn DogHook<R, P>>>,
    pub(crate) config: DogConfig,
}

impl<R, P> DogApp<R, P>
where
    R: Send + 'static,
    P: Send + 'static,
{
    /// Create an empty DogApp instance with no services and no hooks.
    pub fn new() -> Self {
        Self {
            registry: DogServiceRegistry::new(),
            global_hooks: Vec::new(),
            config: DogConfig::new(),
        }
    }

    /// Register a service under its name.
    pub fn register_service<S>(&mut self, name: S, service: Arc<dyn DogService<R, P>>)
    where
        S: Into<String>,
    {
        self.registry.register(name, service);
    }

    /// Register a hook that applies to ALL services in the app.
    pub fn register_global_hook(&mut self, hook: Arc<dyn DogHook<R, P>>) {
        self.global_hooks.push(hook);
    }
}

impl<R, P> DogApp<R, P>
where
    R: Send + 'static,
    P: Send + 'static,
{
    /// Get a service by name, or return an error if it does not exist.
    ///
    /// This mirrors Feathers' `app.service(name)` ergonomics, but uses
    /// Rust's `Result` instead of throwing.
    pub fn service(&self, name: &str) -> Result<&Arc<dyn DogService<R, P>>> {
        self.registry
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("DogService not found: {name}"))
    }

}
impl<R, P> DogApp<R, P>
where
    R: Send + 'static,
    P: Send + 'static,
{
    /// Set a configuration value on the app.
    ///
    /// Equivalent to Feathers `app.set(name, value)`.
    pub fn set<K, V>(&mut self, key: K, value: V)
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.config.set(key, value);
    }

    /// Get a configuration value from the app.
    ///
    /// Equivalent to Feathers `app.get(name)`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.config.get(key)
    }
}


