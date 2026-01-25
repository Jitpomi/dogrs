// OAuth service.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_auth::core::{AuthenticationParams, AuthenticationRequest, AuthenticationResult, JwtOverrides};
use dog_auth::service::AuthenticationService;
use dog_core::errors::DogError;
use dog_core::HookContext;
use serde_json::{Map, Value};

pub struct OAuthCallbackResponse {
    pub auth_result: AuthenticationResult,
    pub location: Option<String>,
}

#[derive(Debug)]
pub struct OAuthError {
    pub message: String,
    pub location: Option<String>,
}

impl std::fmt::Display for OAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for OAuthError {}

#[async_trait]
pub trait OAuthRedirect<P>: Send + Sync
where
    P: Clone + Send + Sync + 'static,
{
    async fn get_redirect(
        &self,
        provider: &str,
        result: &Result<AuthenticationResult>,
        ctx: &mut HookContext<Value, P>,
    ) -> Result<Option<String>>;
}

pub struct OAuthService<P>
where
    P: Clone + Send + Sync + 'static,
{
    pub auth_service: Arc<AuthenticationService<P>>,
    pub redirect: Option<Arc<dyn OAuthRedirect<P>>>,
}

impl<P> OAuthService<P>
where
    P: Clone + Send + Sync + 'static,
{
    pub fn new(auth_service: Arc<AuthenticationService<P>>) -> Self {
        Self {
            auth_service,
            redirect: None,
        }
    }

    pub fn with_redirect(mut self, redirect: Arc<dyn OAuthRedirect<P>>) -> Self {
        self.redirect = Some(redirect);
        self
    }

    pub async fn authenticate_callback(
        &self,
        provider: &str,
        payload: Map<String, Value>,
        params: &AuthenticationParams,
        ctx: &mut HookContext<Value, P>,
        jwt_overrides: Option<JwtOverrides>,
    ) -> Result<OAuthCallbackResponse> {
        if provider.trim().is_empty() {
            return Err(DogError::bad_request("Missing OAuth provider").into_anyhow());
        }

        let authentication = AuthenticationRequest {
            strategy: Some(provider.to_string()),
            data: payload,
        };

        let strategies = vec![provider.to_string()];

        let result = self
            .auth_service
            .create(&authentication, params, ctx, &strategies, jwt_overrides)
            .await;

        let location = match &self.redirect {
            Some(r) => r.get_redirect(provider, &result, ctx).await?,
            None => None,
        };

        match result {
            Ok(auth_result) => Ok(OAuthCallbackResponse { auth_result, location }),
            Err(e) => {
                // We keep this transport-agnostic: adapters can map OAuthError.location to headers.
                let msg = e.to_string();
                Err(anyhow::anyhow!(OAuthError { message: msg, location }))
            }
        }
    }
}