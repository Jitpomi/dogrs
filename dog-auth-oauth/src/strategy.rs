// OAuth strategy.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use anyhow::Result;
use async_trait::async_trait;
use dog_auth::core::{AuthenticationBase, AuthenticationParams, AuthenticationRequest, AuthenticationResult, AuthenticationStrategy};
use dog_core::errors::DogError;
use dog_core::HookContext;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[async_trait]
pub trait OAuthProvider<P>: Send + Sync
where
    P: Clone + Send + Sync + 'static,
{
    fn name(&self) -> &str;

    async fn exchange_code(&self, code: &str, ctx: &mut HookContext<Value, P>) -> Result<String>;

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
    pub profile: Option<Value>,
}

#[derive(Clone)]
pub struct OAuthStrategyOptions<P>
where
    P: Clone + Send + Sync + 'static,
{
    pub default_provider: Option<String>,
    pub providers: HashMap<String, Arc<dyn OAuthProvider<P>>>,
}

impl<P> Default for OAuthStrategyOptions<P>
where
    P: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self {
            default_provider: None,
            providers: HashMap::new(),
        }
    }
}

pub struct OAuthStrategy<P>
where
    P: Clone + Send + Sync + 'static,
{
    auth: Weak<AuthenticationBase<P>>,
    name: String,
    options: OAuthStrategyOptions<P>,
}

impl<P> OAuthStrategy<P>
where
    P: Clone + Send + Sync + 'static,
{
    pub fn new(auth: &Arc<AuthenticationBase<P>>) -> Self {
        Self {
            auth: Arc::downgrade(auth),
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

    fn read_string(data: &Map<String, Value>, key: &str) -> Option<String> {
        data.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
    }

    fn parse_request(&self, authentication: &AuthenticationRequest) -> Result<OAuthAuthenticateData> {
        let provider = Self::read_string(&authentication.data, "provider")
            .or_else(|| self.options.default_provider.clone())
            .ok_or_else(|| DogError::not_authenticated("Missing provider").into_anyhow())?;

        let access_token = Self::read_string(&authentication.data, "accessToken")
            .or_else(|| Self::read_string(&authentication.data, "access_token"));

        let code = Self::read_string(&authentication.data, "code");

        let profile = authentication.data.get("profile").cloned();

        Ok(OAuthAuthenticateData {
            provider,
            access_token,
            code,
            profile,
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
        let svc = ctx.services.service::<Value, P>(service_name)?;

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
        let svc = ctx.services.service::<Value, P>(service_name)?;
        svc.create(&ctx.tenant, Value::Object(data), ctx.params.clone()).await
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
        let svc = ctx.services.service::<Value, P>(service_name)?;
        svc.patch(&ctx.tenant, Some(&id), Value::Object(data), ctx.params.clone()).await
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
    ) -> Result<AuthenticationResult> {
        let auth = self
            .auth
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("AuthenticationBase was dropped"))?;

        let req = self.parse_request(authentication)?;

        let cfg = auth.configuration();
        let provider_cfg_exists = cfg.oauth_providers.contains_key(&req.provider);
        let external = self.options.providers.get(&req.provider).cloned();
        if !provider_cfg_exists && external.is_none() {
            return Err(DogError::not_authenticated("Unknown OAuth provider").into_anyhow());
        }

        if req.access_token.is_none() && req.code.is_none() && req.profile.is_none() {
            return Err(DogError::not_authenticated("Missing OAuth credentials").into_anyhow());
        }

        // Resolve access token and/or profile via external provider implementation.
        let mut access_token = req.access_token.clone();
        let mut profile = req.profile.clone();

        if access_token.is_none() {
            if let (Some(code), Some(provider)) = (req.code.as_deref(), external.as_ref()) {
                access_token = Some(provider.exchange_code(code, ctx).await?);
            }
        }

        if profile.is_none() {
            if let (Some(token), Some(provider)) = (access_token.as_deref(), external.as_ref()) {
                profile = provider.fetch_profile(token, ctx).await?;
            }
        }

        // If entity/service are configured and we have a profile, upsert the entity.
        let mut entity_out: Option<Value> = None;
        if let (Some(service_name), Some(entity_key), Some(profile)) =
            (cfg.service.clone(), cfg.entity.clone(), profile.as_ref())
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

        let mut auth_obj = Map::new();
        auth_obj.insert("strategy".to_string(), Value::String(self.name.clone()));
        auth_obj.insert("provider".to_string(), Value::String(req.provider.clone()));
        if let Some(t) = access_token.clone() {
            auth_obj.insert("accessToken".to_string(), Value::String(t));
        }
        if let Some(c) = req.code.clone() {
            auth_obj.insert("code".to_string(), Value::String(c));
        }

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