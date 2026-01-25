// Authentication service.

use std::sync::Arc;

use anyhow::Result;
use dog_core::errors::DogError;
use dog_core::DogApp;
use dog_core::HookContext;
use serde_json::{json, Value};

use crate::core::{AuthenticationBase, AuthenticationParams, AuthenticationRequest, AuthenticationResult, ConnectionEvent, JwtOverrides};
use crate::options::AuthOptions;

pub const AUTHENTICATION_KEY: &str = "authentication";
pub const AUTHENTICATION_OPTIONS_KEY: &str = "authentication.options";

pub struct AuthenticationService<P>
where
    P: Send + Clone + 'static,
{
    pub base: Arc<AuthenticationBase<P>>,
}

impl<P> AuthenticationService<P>
where
    P: Send + Clone + 'static,
{
    pub fn new(app: DogApp<Value, P>, options: Option<AuthOptions>) -> Result<Self> {
        let base = Arc::new(AuthenticationBase::new(app, AUTHENTICATION_OPTIONS_KEY, options)?);
        Ok(Self { base })
    }

    pub fn install(app: &DogApp<Value, P>, svc: Arc<Self>) {
        app.set(AUTHENTICATION_KEY, Arc::clone(&svc));
    }

    pub fn from_app(app: &DogApp<Value, P>) -> Option<Arc<Self>> {
        app.get::<Arc<Self>>(AUTHENTICATION_KEY)
    }

    pub fn configuration(&self) -> AuthOptions {
        self.base.configuration()
    }

    pub fn register_strategy(
        &self,
        name: impl Into<String>,
        strategy: Arc<dyn crate::core::AuthenticationStrategy<P>>,
    ) {
        self.base.register(name, strategy);
    }

    pub async fn authenticate(
        &self,
        authentication: &AuthenticationRequest,
        params: &AuthenticationParams,
        ctx: &mut HookContext<Value, P>,
        strategies: &[String],
    ) -> Result<AuthenticationResult> {
        self.base.authenticate(authentication, params, ctx, strategies).await
    }

    pub async fn setup_validate(&self) -> Result<()> {
        let cfg = self.configuration();
        cfg.validate()
            .map_err(|e| anyhow::anyhow!(e))?;

        // Basic, Feathers-like sanity check: if JWT is enabled, a secret must be present.
        // (Later, RSA/ECDSA key support can satisfy this instead.)
        if cfg.strategies.contains(&crate::options::AuthStrategy::Jwt) {
            if cfg.jwt.secret.is_none() {
                return Err(anyhow::anyhow!(
                    "A JWT secret must be provided in your authentication configuration"
                ));
            }
        }

        Ok(())
    }

    pub async fn get_payload(
        &self,
        _auth_result: &AuthenticationResult,
        params: &AuthenticationParams,
    ) -> Result<Value> {
        Ok(params.payload.clone().unwrap_or_else(|| json!({})))
    }

    pub async fn create(
        &self,
        authentication: &AuthenticationRequest,
        params: &AuthenticationParams,
        ctx: &mut HookContext<Value, P>,
        strategies: &[String],
        jwt_overrides: Option<JwtOverrides>,
    ) -> Result<AuthenticationResult> {
        if strategies.is_empty() {
            return Err(DogError::not_authenticated(
                "No authentication strategies allowed for creating a JWT (`authStrategies`)",
            )
            .into_anyhow());
        }

        let auth_result = self.authenticate(authentication, params, ctx, strategies).await?;

        if auth_result.get("accessToken").and_then(|v| v.as_str()).is_some() {
            return Ok(auth_result);
        }

        // Minimal Feathers-like behavior: sign the `params.payload` (or empty) as the JWT payload.
        let payload = self.get_payload(&auth_result, params).await?;
        let access_token = self.base.create_access_token(payload, jwt_overrides).await?;

        let mut out = match auth_result {
            Value::Object(m) => m,
            other => {
                let mut m = serde_json::Map::new();
                m.insert("result".to_string(), other);
                m
            }
        };
        out.insert("accessToken".to_string(), Value::String(access_token));

        Ok(Value::Object(out))
    }

    pub async fn remove(
        &self,
        access_token: Option<&str>,
        params: &AuthenticationParams,
        ctx: &mut HookContext<Value, P>,
        strategies: &[String],
    ) -> Result<AuthenticationResult> {
        let token = access_token
            .map(|s| s.to_string())
            .or_else(|| {
                crate::core::extract_bearer_token(&params.headers)
            })
            .ok_or_else(|| DogError::not_authenticated("Invalid access token").into_anyhow())?;

        // Default "logout" behavior: verify (authenticate) the access token.
        let mut data = serde_json::Map::new();
        data.insert("accessToken".to_string(), Value::String(token));
        let auth_req = AuthenticationRequest {
            strategy: Some("jwt".to_string()),
            data,
        };

        self.authenticate(&auth_req, params, ctx, strategies).await
    }

    pub async fn handle_connection(
        &self,
        _event: ConnectionEvent,
        _connection: Arc<dyn std::any::Any + Send + Sync>,
        _auth_result: &AuthenticationResult,
    ) -> Result<()> {
        Ok(())
    }
}