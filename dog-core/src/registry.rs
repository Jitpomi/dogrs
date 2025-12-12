use std::collections::HashMap;
use std::sync::Arc;

use crate::DogService;

/// A simple registry that maps service names to DogService instances.
///
/// This is the core of DogRS: named services that can be called
/// from any transport (HTTP, CLI, jobs, P2P, etc.).
pub struct DogServiceRegistry<R, P = ()>
where
    R: Send + 'static,
    P: Send + 'static,
{
    services: HashMap<String, Arc<dyn DogService<R, P>>>,
}

impl<R, P> DogServiceRegistry<R, P>
where
    R: Send + 'static,
    P: Send + 'static,
{
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
        }
    }

    pub fn register<S>(&mut self, name: S, service: Arc<dyn DogService<R, P>>)
    where
        S: Into<String>,
    {
        self.services.insert(name.into(), service);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn DogService<R, P>>> {
        self.services.get(name)
    }
}

impl<R, P> Default for DogServiceRegistry<R, P>
where
    R: Send + 'static,
    P: Send + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}
