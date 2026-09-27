// Local authentication strategy.

use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use bcrypt::{hash, verify};
use dog_auth::core::{
    AuthenticationBase, AuthenticationParams, AuthenticationRequest, AuthenticationResult,
    AuthenticationStrategy,
};
use dog_core::errors::DogError;
use dog_core::HookContext;
use serde_json::{json, Map, Value};

#[async_trait]
pub trait LocalEntityResolver<P>: Send + Sync
where
    P: Send + Clone + 'static,
{
    async fn resolve_entity(
        &self,
        username: &str,
        ctx: &mut HookContext<Value, P>,
    ) -> Result<Option<Value>>;
}

pub trait LocalEntityQueryBuilder<P>: Send + Sync
where
    P: Send + Clone + 'static,
{
    fn build_find_params(&self, base: &P, username_field: &str, username: &str) -> P;
}

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
    name: String,
    options: LocalStrategyOptions,
    entity_resolver: Option<Arc<dyn LocalEntityResolver<P>>>,
    dummy_hash: Arc<std::sync::OnceLock<String>>,
    entity_query_builder: Option<Arc<dyn LocalEntityQueryBuilder<P>>>,
}

impl<P> Default for LocalStrategy<P>
where
    P: Send + Clone + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<P> LocalStrategy<P>
where
    P: Send + Clone + 'static,
{
    pub fn new() -> Self {
        Self {
            name: "local".to_string(),
            options: LocalStrategyOptions::default(),
            entity_resolver: None,
            dummy_hash: Arc::new(std::sync::OnceLock::new()),
            entity_query_builder: None,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn with_options(mut self, options: LocalStrategyOptions) -> Self {
        self.options = options;
        self.dummy_hash = Arc::new(std::sync::OnceLock::new());
        self
    }

    pub fn with_entity_resolver(mut self, resolver: Arc<dyn LocalEntityResolver<P>>) -> Self {
        self.entity_resolver = Some(resolver);
        self
    }

    pub fn with_entity_query_builder(
        mut self,
        builder: Arc<dyn LocalEntityQueryBuilder<P>>,
    ) -> Self {
        self.entity_query_builder = Some(builder);
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
        anyhow::ensure!(
            (4..=16).contains(&self.options.hash_size),
            "bcrypt cost must be between 4 and 16"
        );
        Ok(())
    }

    pub async fn hash_password(&self, password: &str) -> Result<String> {
        self.verify_configuration()?;
        validate_password(password)?;
        let password = password.to_owned();
        let cost = self.options.hash_size;
        let permit = password_workers()
            .try_acquire_owned()
            .map_err(|_| DogError::unavailable("Password workers busy").into_anyhow())?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            hash(password, cost)
        })
        .await?
        .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    fn get_required_str(
        data: &Map<String, Value>,
        key: &str,
        error_message: &str,
    ) -> Result<String> {
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
        let mut parts = password_field_path
            .split('.')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .peekable();
        let mut current = &mut entity;
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                if let Some(map) = current.as_object_mut() {
                    map.remove(part);
                }
                break;
            }
            match current.get_mut(part) {
                Some(child) => current = child,
                None => break,
            }
        }
        entity
    }

    async fn find_entity(
        &self,
        ctx: &mut HookContext<Value, P>,
        service_name: &str,
        username: &str,
    ) -> Result<Option<Value>> {
        if username.trim().is_empty() {
            return Ok(None);
        }

        let svc = ctx.services.service(service_name)?;

        // If a query builder is provided, allow the app/adaptor to inject an efficient query/limit
        // into the params type (e.g. for Mongo/Postgres adapters).
        let params = if let Some(builder) = self.entity_query_builder.as_ref() {
            builder.build_find_params(&ctx.params, &self.options.entity_username_field, username)
        } else {
            ctx.params.clone()
        };

        // Fallback remains safe: we still verify the username match.
        let all = svc.find(&ctx.tenant, params).await?;

        for entity in all {
            let matches = entity
                .get(&self.options.entity_username_field)
                .and_then(|v| v.as_str())
                .map(|s| s == username)
                .unwrap_or(false);
            if matches {
                return Ok(Some(entity));
            }
        }

        Ok(None)
    }

    async fn compare_password(&self, entity: Option<&Value>, password: &str) -> Result<()> {
        let hash_val = entity
            .and_then(|e| Self::get_by_path(e, &self.options.entity_password_field))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let password = password.to_owned();
        let cost = self.options.hash_size;
        let dummy = self.dummy_hash.clone();
        let permit = password_workers()
            .try_acquire_owned()
            .map_err(|_| DogError::unavailable("Password workers busy").into_anyhow())?;
        let ok = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            // Reject malformed or excessive-cost stored hashes without allowing
            // their fast failure to skip the dummy password work.
            let valid = hash_val.as_deref().filter(|h| {
                bcrypt::HashParts::from_str(h)
                    .is_ok_and(|parts| (4..=16).contains(&parts.get_cost()))
            });
            let hash_val = match valid {
                Some(h) => h,
                None => {
                    if dummy.get().is_none() {
                        let _ = dummy.set(hash("dogrs-dummy-password-not-an-account", cost)?);
                    }
                    dummy.get().expect("dummy hash initialized")
                }
            };
            Ok::<bool, bcrypt::BcryptError>(verify(password, hash_val)? && valid.is_some())
        })
        .await?
        .map_err(|_| DogError::not_authenticated(&self.options.error_message).into_anyhow())?;
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
        auth: &AuthenticationBase<P>,
    ) -> Result<AuthenticationResult> {
        self.verify_configuration()?;

        let cfg = auth.configuration();
        let service_name = cfg.service.clone();
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

        validate_password(&password)
            .map_err(|_| DogError::not_authenticated(&self.options.error_message).into_anyhow())?;

        let entity = if let Some(resolver) = self.entity_resolver.as_ref() {
            resolver.resolve_entity(&username, ctx).await?
        } else {
            let service_name = service_name.ok_or_else(|| {
                DogError::not_authenticated("Local strategy requires authentication.service")
                    .into_anyhow()
            })?;
            self.find_entity(ctx, &service_name, &username).await?
        };
        self.compare_password(entity.as_ref(), &password).await?;
        let entity = entity.ok_or_else(|| {
            DogError::not_authenticated(&self.options.error_message).into_anyhow()
        })?;

        let entity = Self::strip_password(entity, &self.options.entity_password_field);

        Ok(json!({
            "authentication": { "strategy": self.name },
            entity_key: entity
        }))
    }
}

fn password_workers() -> Arc<tokio::sync::Semaphore> {
    static WORKERS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> = std::sync::OnceLock::new();
    WORKERS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(16)))
        .clone()
}

fn validate_password(password: &str) -> Result<()> {
    if password.trim().is_empty() || password.len() > 72 {
        return Err(DogError::bad_request(
            "Password must contain 1 to 72 UTF-8 bytes and not be blank",
        )
        .into_anyhow());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn missing_user_and_malformed_hash_still_perform_password_work() {
        let strategy = LocalStrategy::<()>::new().with_options(LocalStrategyOptions {
            hash_size: 4,
            ..Default::default()
        });
        assert!(strategy.compare_password(None, "wrong").await.is_err());
        assert!(strategy.dummy_hash.get().is_some());
        let malformed = serde_json::json!({"password":"not-a-hash"});
        assert!(strategy
            .compare_password(Some(&malformed), "wrong")
            .await
            .is_err());
        let good =
            serde_json::json!({"password": strategy.hash_password("correct").await.unwrap()});
        assert!(strategy
            .compare_password(Some(&good), "correct")
            .await
            .is_ok());
        assert!(strategy
            .compare_password(Some(&good), "wrong")
            .await
            .is_err());

        let permit = password_workers().acquire_many_owned(16).await.unwrap();
        let strategy = LocalStrategy::<()>::new();
        let error = strategy.hash_password("valid").await.unwrap_err();
        assert_eq!(error.downcast_ref::<DogError>().unwrap().code(), 503);
        drop(permit);
    }
}
