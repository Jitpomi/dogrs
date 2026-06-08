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

// ---------------------------------------------------------------------------
// Type alias for the configured client with both auth_uri AND token_uri set.
// oauth2 5.x uses type-state generics: each endpoint tracks Set/NotSet.
// Parameters (in order): HasAuthUrl, HasDeviceAuthUrl, HasIntrospectionUrl,
//                         HasRevocationUrl, HasTokenUrl
// ---------------------------------------------------------------------------
type ConfiguredBasicClient = BasicClient<
    EndpointSet,    // HasAuthUrl    — set via .set_auth_uri()
    EndpointNotSet, // HasDeviceAuthUrl
    EndpointNotSet, // HasIntrospectionUrl
    EndpointNotSet, // HasRevocationUrl
    EndpointSet,    // HasTokenUrl   — set via .set_token_uri()
>;

pub struct OAuth2ClientConfig {
    pub name: String,
    pub client_id: String,
    pub client_secret: String,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub userinfo_url: Option<String>,
}

pub struct OAuth2AuthorizationCodeProvider<P>
where
    P: Clone + Send + Sync + 'static,
{
    name: String,
    client: ConfiguredBasicClient,
    scopes: Vec<String>,
    userinfo_url: Option<String>,
    _marker: PhantomData<fn() -> P>,
}

impl<P> OAuth2AuthorizationCodeProvider<P>
where
    P: Clone + Send + Sync + 'static,
{
    pub fn new(config: OAuth2ClientConfig) -> Result<Self> {
        // oauth2 5.x: BasicClient::new() takes only ClientId; other fields via builders.
        // Each set_* call changes the type-state, giving us ConfiguredBasicClient.
        let client = BasicClient::new(ClientId::new(config.client_id))
            .set_client_secret(ClientSecret::new(config.client_secret))
            .set_auth_uri(AuthUrl::new(config.auth_url)?)
            .set_token_uri(TokenUrl::new(config.token_url)?)
            .set_redirect_uri(RedirectUrl::new(config.redirect_uri)?);

        Ok(Self {
            name: config.name,
            client,
            scopes: config.scopes,
            userinfo_url: config.userinfo_url,
            _marker: PhantomData,
        })
    }

    pub fn authorize_url(&self) -> String {
        // oauth2 5.x with EndpointSet: authorize_url() returns AuthorizationRequest directly
        // (infallible — no Result). Use .url() to extract the (Url, CsrfToken) pair.
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
        // oauth2 5.x with EndpointSet: exchange_code() returns CodeTokenRequest (not Result).
        // request_async takes a &reqwest::Client (implements AsyncHttpClient).
        let http_client = reqwest::Client::new();
        let token = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .request_async(&http_client)
            .await?;

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

        let client = reqwest::Client::new();
        let profile = client
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
