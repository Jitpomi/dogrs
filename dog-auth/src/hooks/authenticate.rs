// Authenticate hook.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::errors::DogError;
use dog_core::hooks::DogBeforeHook;
use dog_core::HookContext;
use serde_json::{Map, Value};

use crate::core::{extract_bearer_token, AuthenticationParams, AuthenticationRequest, AuthenticationResult};
use crate::service::AuthenticationService;

pub trait AuthenticateHookParams: Clone + Send + Sync {
    fn provider(&self) -> Option<&str>;
    fn headers(&self) -> &HashMap<String, String>;
    fn authentication(&self) -> Option<&AuthenticationRequest>;
    fn authenticated(&self) -> bool;

    fn set_authenticated(&mut self, v: bool);
    fn set_auth_result(&mut self, v: AuthenticationResult);
}

#[derive(Clone, Debug, Default)]
pub struct AuthParams<P> {
    pub inner: P,
    pub provider: Option<String>,
    pub headers: HashMap<String, String>,
    pub authentication: Option<AuthenticationRequest>,
    pub authenticated: bool,
    pub auth_result: Option<AuthenticationResult>,
    pub connection: Option<Arc<dyn std::any::Any + Send + Sync>>,
}

impl<P> AuthenticateHookParams for AuthParams<P>
where
    P: Clone + Send + Sync,
{
    fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }

    fn headers(&self) -> &HashMap<String, String> {
        &self.headers
    }

    fn authentication(&self) -> Option<&AuthenticationRequest> {
        self.authentication.as_ref()
    }

    fn authenticated(&self) -> bool {
        self.authenticated
    }

    fn set_authenticated(&mut self, v: bool) {
        self.authenticated = v;
    }

    fn set_auth_result(&mut self, v: AuthenticationResult) {
        self.auth_result = Some(v);
    }
}

pub struct AuthenticateHook<P>
where
    P: AuthenticateHookParams + 'static,
{
    auth_service: Arc<AuthenticationService<P>>,
    strategies: Vec<String>,
}

impl<P> AuthenticateHook<P>
where
    P: AuthenticateHookParams + 'static,
{
    pub fn new(auth_service: Arc<AuthenticationService<P>>, strategies: Vec<String>) -> Self {
        Self {
            auth_service,
            strategies,
        }
    }

    pub fn from_app(app: &dog_core::DogApp<Value, P>, strategies: Vec<String>) -> Result<Self>
    where
        P: Clone + Send + Sync,
    {
        let auth_service = AuthenticationService::from_app(app)
            .ok_or_else(|| DogError::not_authenticated("Could not find a valid authentication service").into_anyhow())?;
        Ok(Self::new(auth_service, strategies))
    }
}

#[async_trait]
impl<P> DogBeforeHook<Value, P> for AuthenticateHook<P>
where
    P: AuthenticateHookParams + Clone + Send + Sync + 'static,
{
    async fn run(&self, ctx: &mut HookContext<Value, P>) -> Result<()> {
        if ctx.params.authenticated() {
            return Ok(());
        }

        let provider = ctx.params.provider().unwrap_or("");
        if provider.trim().is_empty() {
            // Internal call: allow through.
            return Ok(());
        }

        let auth_req = if let Some(req) = ctx.params.authentication() {
            req.clone()
        } else if let Some(token) = extract_bearer_token(ctx.params.headers()) {
            let mut data = Map::new();
            data.insert("accessToken".to_string(), Value::String(token));
            AuthenticationRequest {
                strategy: Some("jwt".to_string()),
                data,
            }
        } else {
            return Err(DogError::not_authenticated("Not authenticated").into_anyhow());
        };

        if self.strategies.is_empty() {
            return Err(anyhow::anyhow!("The authenticate hook needs at least one allowed strategy"));
        }

        let auth_params = AuthenticationParams {
            payload: None,
            jwt_options: None,
            auth_strategies: Some(self.strategies.clone()),
            secret: None,
            headers: ctx.params.headers().clone(),
        };

        let auth_result = self
            .auth_service
            .authenticate(&auth_req, &auth_params, ctx, &self.strategies)
            .await?;

        ctx.params.set_auth_result(auth_result);
        ctx.params.set_authenticated(true);

        Ok(())
    }
}