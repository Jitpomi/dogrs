// Local authentication strategy.

use std::sync::{Arc, Weak};

use anyhow::Result;
use async_trait::async_trait;
use bcrypt::{hash, verify};
use dog_auth::core::{AuthenticationBase, AuthenticationParams, AuthenticationRequest, AuthenticationResult, AuthenticationStrategy};
use dog_core::errors::DogError;
use dog_core::HookContext;
use serde_json::{json, Map, Value};

#[derive(Clone, Debug)]
pub struct LocalStrategyOptions {
    pub username_field: String,
    pub password_field: String,

    pub entity_username_field: String,
    pub entity_password_field: String,

    pub error_message: String,
    pub hash_size: u32,
}

impl Default for LocalStrategyOptions {
    fn default() -> Self {
        Self {
            username_field: "email".to_string(),
            password_field: "password".to_string(),
            entity_username_field: "email".to_string(),
            entity_password_field: "password".to_string(),
            error_message: "Invalid login".to_string(),
            hash_size: 10,
        }
    }
}

pub struct LocalStrategy<P>
where
    P: Send + Clone + 'static,
{
    auth: Weak<AuthenticationBase<P>>,
    name: String,
    options: LocalStrategyOptions,
}

impl<P> LocalStrategy<P>
where
    P: Send + Clone + 'static,
{
    pub fn new(auth: &Arc<AuthenticationBase<P>>) -> Self {
        Self {
            auth: Arc::downgrade(auth),
            name: "local".to_string(),
            options: LocalStrategyOptions::default(),
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn with_options(mut self, options: LocalStrategyOptions) -> Self {
        self.options = options;
        self
    }

    pub fn verify_configuration(&self) -> Result<()> {
        if self.options.username_field.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "'{}' authentication strategy requires a 'username_field' setting",
                self.name
            ));
        }
        if self.options.password_field.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "'{}' authentication strategy requires a 'password_field' setting",
                self.name
            ));
        }
        Ok(())
    }

    pub async fn hash_password(&self, password: &str) -> Result<String> {
        hash(password, self.options.hash_size)
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    fn get_required_str(data: &Map<String, Value>, key: &str, error_message: &str) -> Result<String> {
        let v = data
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| DogError::not_authenticated(error_message).into_anyhow())?;
        Ok(v)
    }

    fn get_by_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
        let mut cur = value;
        for part in path.split('.').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            cur = cur.get(part)?;
        }
        Some(cur)
    }

    fn strip_password(mut entity: Value, password_field_path: &str) -> Value {
        // Only supports direct object key stripping; dotted paths are left intact.
        if !password_field_path.contains('.') {
            if let Value::Object(ref mut map) = entity {
                map.remove(password_field_path);
            }
        }
        entity
    }

    async fn find_entity(
        &self,
        ctx: &mut HookContext<Value, P>,
        service_name: &str,
        username: &str,
    ) -> Result<Value> {
        if username.trim().is_empty() {
            return Err(DogError::not_authenticated(&self.options.error_message).into_anyhow());
        }

        let svc = ctx.services.service::<Value, P>(service_name)?;

        // Minimal transport-agnostic lookup: fetch all and filter in memory.
        // Production services should implement a query-capable params type or a custom method.
        let all = svc.find(&ctx.tenant, ctx.params.clone()).await?;

        for entity in all {
            let matches = entity
                .get(&self.options.entity_username_field)
                .and_then(|v| v.as_str())
                .map(|s| s == username)
                .unwrap_or(false);
            if matches {
                return Ok(entity);
            }
        }

        Err(DogError::not_authenticated(&self.options.error_message).into_anyhow())
    }

    async fn compare_password(&self, entity: &Value, password: &str) -> Result<()> {
        let hash_val = Self::get_by_path(entity, &self.options.entity_password_field)
            .and_then(|v| v.as_str());

        let Some(hash_val) = hash_val else {
            return Err(DogError::not_authenticated(&self.options.error_message).into_anyhow());
        };

        let ok = verify(password, hash_val)
            .map_err(|e| DogError::not_authenticated(e.to_string()).into_anyhow())?;
        if !ok {
            return Err(DogError::not_authenticated(&self.options.error_message).into_anyhow());
        }
        Ok(())
    }
}

#[async_trait]
impl<P> AuthenticationStrategy<P> for LocalStrategy<P>
where
    P: Send + Clone + 'static,
{
    async fn authenticate(
        &self,
        authentication: &AuthenticationRequest,
        _params: &AuthenticationParams,
        ctx: &mut HookContext<Value, P>,
    ) -> Result<AuthenticationResult> {
        self.verify_configuration()?;

        let auth = self
            .auth
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("AuthenticationBase was dropped"))?;

        let cfg = auth.configuration();
        let service_name = cfg.service.clone().ok_or_else(|| {
            DogError::not_authenticated("Local strategy requires authentication.service").into_anyhow()
        })?;
        let entity_key = cfg.entity.clone().unwrap_or_else(|| "user".to_string());

        let username = Self::get_required_str(
            &authentication.data,
            &self.options.username_field,
            &self.options.error_message,
        )?;
        let password = Self::get_required_str(
            &authentication.data,
            &self.options.password_field,
            &self.options.error_message,
        )?;

        let entity = self.find_entity(ctx, &service_name, &username).await?;
        self.compare_password(&entity, &password).await?;

        let entity = Self::strip_password(entity, &self.options.entity_password_field);

        Ok(json!({
            "authentication": { "strategy": self.name },
            entity_key: entity
        }))
    }
}