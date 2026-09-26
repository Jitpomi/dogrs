// OAuth strategy.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_auth::core::{
    AuthenticationBase, AuthenticationParams, AuthenticationRequest, AuthenticationResult,
    AuthenticationStrategy,
};
use dog_core::errors::DogError;
use dog_core::HookContext;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[async_trait]
pub trait OAuthEntityResolver<P>: Send + Sync
where
    P: Clone + Send + Sync + 'static,
{
    async fn resolve_entity(
        &self,
        provider: &str,
        profile: &Value,
        ctx: &mut HookContext<Value, P>,
    ) -> Result<Option<Value>>;
}

#[async_trait]
pub trait OAuthProvider<P>: Send + Sync
where
    P: Clone + Send + Sync + 'static,
{
    fn name(&self) -> &str;

    async fn exchange_code(
        &self,
        code: &str,
        state: Option<&str>,
        ctx: &mut HookContext<Value, P>,
    ) -> Result<String>;

    async fn fetch_profile(
        &self,
        _access_token: &str,
        _ctx: &mut HookContext<Value, P>,
    ) -> Result<Option<Value>> {
        Ok(None)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OAuthAuthenticateData {
    pub provider: String,
    pub access_token: Option<String>,
    pub code: Option<String>,
    pub state: Option<String>,
}

#[derive(Clone)]
pub struct OAuthStrategyOptions<P>
where
    P: Clone + Send + Sync + 'static,
{
    pub default_provider: Option<String>,
    pub providers: HashMap<String, Arc<dyn OAuthProvider<P>>>,
    pub entity_resolver: Option<Arc<dyn OAuthEntityResolver<P>>>,
}

impl<P> Default for OAuthStrategyOptions<P>
where
    P: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self {
            default_provider: None,
            providers: HashMap::new(),
            entity_resolver: None,
        }
    }
}

pub struct OAuthStrategy<P>
where
    P: Clone + Send + Sync + 'static,
{
    name: String,
    options: OAuthStrategyOptions<P>,
}

impl<P> Default for OAuthStrategy<P>
where
    P: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<P> OAuthStrategy<P>
where
    P: Clone + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            name: "oauth".to_string(),
            options: OAuthStrategyOptions::default(),
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn with_options(mut self, options: OAuthStrategyOptions<P>) -> Self {
        self.options = options;
        self
    }

    pub fn register_provider(mut self, provider: Arc<dyn OAuthProvider<P>>) -> Self {
        self.options
            .providers
            .insert(provider.name().to_string(), provider);
        self
    }

    pub fn with_entity_resolver(mut self, resolver: Arc<dyn OAuthEntityResolver<P>>) -> Self {
        self.options.entity_resolver = Some(resolver);
        self
    }

    fn read_string(data: &Map<String, Value>, key: &str) -> Option<String> {
        data.get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    fn parse_request(
        &self,
        authentication: &AuthenticationRequest,
    ) -> Result<OAuthAuthenticateData> {
        let provider = Self::read_string(&authentication.data, "provider")
            .or_else(|| self.options.default_provider.clone())
            .ok_or_else(|| DogError::not_authenticated("Missing provider").into_anyhow())?;

        let access_token = Self::read_string(&authentication.data, "accessToken")
            .or_else(|| Self::read_string(&authentication.data, "access_token"));

        let code = Self::read_string(&authentication.data, "code");

        if authentication.data.contains_key("profile") {
            return Err(DogError::bad_request(
                "OAuth profiles must be fetched from the configured provider",
            )
            .into_anyhow());
        }
        let state = Self::read_string(&authentication.data, "state");

        Ok(OAuthAuthenticateData {
            provider,
            access_token,
            code,
            state,
        })
    }

    fn profile_id(provider: &str, profile: &Value) -> Option<String> {
        // Common conventions: sub, id, and <provider>Id.
        profile
            .get("sub")
            .or_else(|| profile.get("id"))
            .or_else(|| profile.get(format!("{provider}Id").as_str()))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    async fn find_entity(
        &self,
        ctx: &mut HookContext<Value, P>,
        service_name: &str,
        provider: &str,
        profile: &Value,
    ) -> Result<Option<Value>> {
        let Some(pid) = Self::profile_id(provider, profile) else {
            return Ok(None);
        };

        let key = format!("{provider}Id");
        let svc = ctx.services.service(service_name)?;

        // Minimal lookup: find all and filter.
        let all = svc.find(&ctx.tenant, ctx.params.clone()).await?;
        for entity in all {
            if entity.get(&key).and_then(|v| v.as_str()) == Some(pid.as_str()) {
                return Ok(Some(entity));
            }
        }

        Ok(None)
    }

    async fn create_entity(
        &self,
        ctx: &mut HookContext<Value, P>,
        service_name: &str,
        provider: &str,
        profile: &Value,
    ) -> Result<Value> {
        let mut data = Map::new();
        let Some(pid) = Self::profile_id(provider, profile) else {
            return Err(DogError::not_authenticated("Missing profile id").into_anyhow());
        };
        data.insert(format!("{provider}Id"), Value::String(pid));
        let svc = ctx.services.service(service_name)?;
        svc.create(&ctx.tenant, Value::Object(data), ctx.params.clone())
            .await
    }

    async fn update_entity(
        &self,
        ctx: &mut HookContext<Value, P>,
        service_name: &str,
        existing: &Value,
        provider: &str,
        profile: &Value,
    ) -> Result<Value> {
        let Some(id) = existing
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
        else {
            // If we can't patch, fall back to returning the existing entity.
            return Ok(existing.clone());
        };

        let mut data = Map::new();
        if let Some(pid) = Self::profile_id(provider, profile) {
            data.insert(format!("{provider}Id"), Value::String(pid));
        }
        let svc = ctx.services.service(service_name)?;
        svc.patch(
            &ctx.tenant,
            Some(&id),
            Value::Object(data),
            ctx.params.clone(),
        )
        .await
    }
}

#[async_trait]
impl<P> AuthenticationStrategy<P> for OAuthStrategy<P>
where
    P: Clone + Send + Sync + 'static,
{
    async fn authenticate(
        &self,
        authentication: &AuthenticationRequest,
        _params: &AuthenticationParams,
        ctx: &mut HookContext<Value, P>,
        auth: &AuthenticationBase<P>,
    ) -> Result<AuthenticationResult> {
        let req = self.parse_request(authentication)?;

        let cfg = auth.configuration();
        let provider =
            self.options.providers.get(&req.provider).ok_or_else(|| {
                DogError::not_authenticated("Unknown OAuth provider").into_anyhow()
            })?;
        let access_token = match (req.code.as_deref(), req.access_token.as_deref()) {
            (Some(code), None) => provider
                .exchange_code(code, req.state.as_deref(), ctx)
                .await
                .map_err(map_oauth_provider_error)?,
            (None, Some(token)) => token.to_string(),
            _ => {
                return Err(DogError::not_authenticated(
                    "Supply exactly one OAuth code or access token",
                )
                .into_anyhow())
            }
        };
        if access_token.trim().is_empty() {
            return Err(DogError::not_authenticated("Empty OAuth access token").into_anyhow());
        }
        let profile = provider
            .fetch_profile(&access_token, ctx)
            .await
            .map_err(map_oauth_provider_error)?
            .filter(|profile| {
                Self::profile_id(&req.provider, profile).is_some_and(|id| !id.trim().is_empty())
            })
            .ok_or_else(|| {
                DogError::not_authenticated("Provider did not return a verified identity")
                    .into_anyhow()
            })?;
        let profile = Some(profile);

        // If entity/service are configured and we have a profile, upsert the entity.
        let mut entity_out: Option<Value> = None;
        if let Some(profile) = profile.as_ref() {
            if let (Some(entity_key), Some(resolver)) =
                (cfg.entity.clone(), self.options.entity_resolver.as_ref())
            {
                if let Some(entity) = resolver.resolve_entity(&req.provider, profile, ctx).await? {
                    entity_out = Some(json!({ entity_key: entity }));
                }
            } else if let (Some(service_name), Some(entity_key)) =
                (cfg.service.clone(), cfg.entity.clone())
            {
                let existing = self
                    .find_entity(ctx, &service_name, &req.provider, profile)
                    .await?;
                let entity = if let Some(existing) = existing {
                    self.update_entity(ctx, &service_name, &existing, &req.provider, profile)
                        .await?
                } else {
                    self.create_entity(ctx, &service_name, &req.provider, profile)
                        .await?
                };
                entity_out = Some(json!({ entity_key: entity }));
            }
        }

        if cfg.entity.is_some() && entity_out.is_none() {
            return Err(DogError::not_authenticated(
                "OAuth identity has no authorized local entity",
            )
            .into_anyhow());
        }

        let mut auth_obj = Map::new();
        auth_obj.insert("strategy".to_string(), Value::String(self.name.clone()));
        auth_obj.insert("provider".to_string(), Value::String(req.provider.clone()));
        let mut out = json!({
            "authentication": Value::Object(auth_obj),
        });

        if let Some(profile) = profile {
            if let Some(map) = out.as_object_mut() {
                map.insert("profile".to_string(), profile);
            }
        }

        if let Some(entity) = entity_out {
            if let (Some(map), Some(entity_map)) = (out.as_object_mut(), entity.as_object()) {
                for (k, v) in entity_map {
                    map.insert(k.clone(), v.clone());
                }
            }
        }

        Ok(out)
    }
}

fn map_oauth_provider_error(e: anyhow::Error) -> anyhow::Error {
    // We keep this provider-agnostic by inspecting error chain text.
    // If we can identify a common OAuth failure, return a DogError::bad_request so HTTP adapters
    // produce a clear 400 rather than a generic 500.
    let mut hay = String::new();
    for cause in e.chain() {
        hay.push_str(&cause.to_string().to_lowercase());
        hay.push('\n');
    }

    if hay.contains("invalid_grant")
        || hay.contains("code was already redeemed")
        || hay.contains("already been redeemed")
        || hay.contains("authorization code") && hay.contains("already")
    {
        return DogError::bad_request("OAuth code is invalid/expired or already used")
            .into_anyhow();
    }

    if hay.contains("redirect_uri_mismatch")
        || (hay.contains("redirect") && hay.contains("mismatch"))
    {
        return DogError::bad_request("OAuth redirect_uri mismatch").into_anyhow();
    }

    if hay.contains("invalid_client") {
        return DogError::bad_request("OAuth client configuration is invalid").into_anyhow();
    }

    if hay.contains("unauthorized_client") {
        return DogError::bad_request("OAuth client is not authorized").into_anyhow();
    }

    e
}
