use std::marker::PhantomData;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::HookContext;
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use serde_json::Value;

use crate::strategy::OAuthProvider;

/// A `BasicClient` with auth URL and token URL configured (type-state pattern in oauth2 5.0).
/// HasAuthUrl=EndpointSet, HasDeviceAuthUrl=EndpointNotSet,
/// HasIntrospectionUrl=EndpointNotSet, HasRevocationUrl=EndpointNotSet,
/// HasTokenUrl=EndpointSet
type ConfiguredClient = BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

pub struct OAuth2AuthorizationCodeProvider<P>
where
    P: Clone + Send + Sync + 'static,
{
    name: String,
    client: ConfiguredClient,
    scopes: Vec<String>,
    userinfo_url: Option<String>,
    http_client: reqwest::Client,
    _marker: PhantomData<fn() -> P>,
}

impl<P> OAuth2AuthorizationCodeProvider<P>
where
    P: Clone + Send + Sync + 'static,
{
    pub fn new(
        name: impl Into<String>,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        auth_url: impl Into<String>,
        token_url: impl Into<String>,
        redirect_uri: impl Into<String>,
        scopes: Vec<String>,
        userinfo_url: Option<String>,
    ) -> Result<Self> {
        // oauth2 5.0: Client::new takes only ClientId; other endpoints are set via builder methods.
        // Each set_*_uri call changes the type param from EndpointNotSet to EndpointSet.
        let client = BasicClient::new(ClientId::new(client_id.into()))
            .set_client_secret(ClientSecret::new(client_secret.into()))
            .set_auth_uri(AuthUrl::new(auth_url.into())?)
            .set_token_uri(TokenUrl::new(token_url.into())?)
            .set_redirect_uri(RedirectUrl::new(redirect_uri.into())?);

        Ok(Self {
            name: name.into(),
            client,
            scopes,
            userinfo_url,
            http_client: reqwest::Client::new(),
            _marker: PhantomData,
        })
    }

    pub fn authorize_url(&self) -> String {
        let mut req = self.client.authorize_url(CsrfToken::new_random);
        for s in &self.scopes {
            req = req.add_scope(Scope::new(s.clone()));
        }
        let (url, _csrf) = req.url();
        url.to_string()
    }
}

#[async_trait]
impl<P> OAuthProvider<P> for OAuth2AuthorizationCodeProvider<P>
where
    P: Clone + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    async fn exchange_code(&self, code: &str, _ctx: &mut HookContext<Value, P>) -> Result<String> {
        // oauth2 5.0: pass reqwest::Client directly instead of the removed async_http_client fn
        let token = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .request_async(&self.http_client)
            .await
            .map_err(|e| anyhow::anyhow!("Token exchange failed: {e}"))?;

        Ok(token.access_token().secret().to_string())
    }

    async fn fetch_profile(
        &self,
        access_token: &str,
        _ctx: &mut HookContext<Value, P>,
    ) -> Result<Option<Value>> {
        let Some(url) = self.userinfo_url.as_deref() else {
            return Ok(None);
        };

        let profile = self.http_client
            .get(url)
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;

        Ok(Some(profile))
    }
}
