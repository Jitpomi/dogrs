// Authentication strategies.

use std::sync::Arc;

use anyhow::Result;
use dog_core::app::ServiceHandle;
use dog_core::DogApp;
use serde_json::Value;

use crate::core::AuthenticationBase;

pub struct AuthenticationBaseStrategy<P>
where
    P: Send + Clone + 'static,
{
    authentication: Option<Arc<AuthenticationBase<P>>>,
    app: Option<DogApp<Value, P>>,
    name: Option<String>,
}

impl<P> Default for AuthenticationBaseStrategy<P>
where
    P: Send + Clone + 'static,
{
    fn default() -> Self {
        Self {
            authentication: None,
            app: None,
            name: None,
        }
    }
}

impl<P> AuthenticationBaseStrategy<P>
where
    P: Send + Clone + 'static,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_authentication(&mut self, auth: Arc<AuthenticationBase<P>>) {
        self.authentication = Some(auth);
    }

    pub fn set_application(&mut self, app: DogApp<Value, P>) {
        self.app = Some(app);
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = Some(name.into());
    }

    pub fn configuration(&self) -> Option<Value> {
        let auth = self.authentication.as_ref()?;
        let name = self.name.as_deref()?;

        // Feathers uses `authentication.configuration[this.name]`.
        // In DogRS, configuration is strongly typed (AuthOptions). We emulate the Feathers
        // lookup by serializing to JSON and indexing by the strategy name.
        let v = serde_json::to_value(auth.configuration()).ok()?;
        match v {
            Value::Object(map) => map.get(name).cloned(),
            _ => None,
        }
    }

    pub fn entity_service(&self) -> Result<Option<ServiceHandle<Value, P>>> {
        let Some(app) = self.app.as_ref() else {
            return Ok(None);
        };

        let Some(cfg) = self.configuration() else {
            return Ok(None);
        };

        let Some(service_name) = cfg.get("service").and_then(|v| v.as_str()) else {
            return Ok(None);
        };

        if service_name.trim().is_empty() {
            return Ok(None);
        }

        Ok(Some(app.service(service_name)?))
    }
}