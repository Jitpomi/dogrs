use std::sync::Arc;

use crate::{
    DogHook, DogService, DogServiceRegistry,
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
pub struct DogApp<R, P = ()> {
    pub(crate) registry: DogServiceRegistry<R, P>,
    pub(crate) global_hooks: Vec<Arc<dyn DogHook<R, P>>>,
}

impl<R, P> DogApp<R, P> {
    /// Create an empty DogApp instance with no services and no hooks.
    pub fn new() -> Self {
        Self {
            registry: DogServiceRegistry::new(),
            global_hooks: Vec::new(),
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
