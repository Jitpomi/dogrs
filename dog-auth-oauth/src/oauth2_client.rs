use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::HookContext;
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, TokenUrl,
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

/// Store `state` and `code_verifier` server-side, bound to the initiating browser
/// session with a short expiration. Never return the verifier to an API caller.
pub struct OAuthAuthorization {
    pub url: String,
    pub state: String,
    pub code_verifier: String,
}

#[async_trait]
pub trait OAuthCallbackVerifier<P>: Send + Sync
where
    P: Clone + Send + Sync + 'static,
{
    /// Validate browser/session binding and expiration, atomically consume the
    /// one-use state, then return its stored PKCE verifier. Reject missing/replayed state.
    async fn consume(&self, state: &str, ctx: &mut HookContext<Value, P>) -> Result<String>;
}

pub struct OAuth2AuthorizationCodeProvider<P>
where
    P: Clone + Send + Sync + 'static,
{
    name: String,
    client: ConfiguredBasicClient,
    scopes: Vec<String>,
    userinfo_url: Option<String>,
    verifier: Arc<dyn OAuthCallbackVerifier<P>>,
    http: reqwest::Client,
}

impl<P> OAuth2AuthorizationCodeProvider<P>
where
    P: Clone + Send + Sync + 'static,
{
    pub fn new(
        config: OAuth2ClientConfig,
        verifier: Arc<dyn OAuthCallbackVerifier<P>>,
    ) -> Result<Self> {
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
            verifier,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(15))
                .build()?,
        })
    }

    pub fn authorize_url(&self) -> OAuthAuthorization {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let mut req = self
            .client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(challenge);
        for scope in &self.scopes {
            req = req.add_scope(Scope::new(scope.clone()));
        }
        let (url, state) = req.url();
        OAuthAuthorization {
            url: url.to_string(),
            state: state.secret().clone(),
            code_verifier: verifier.secret().clone(),
        }
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

    async fn exchange_code(
        &self,
        code: &str,
        state: Option<&str>,
        ctx: &mut HookContext<Value, P>,
    ) -> Result<String> {
        // oauth2 5.x with EndpointSet: exchange_code() returns CodeTokenRequest (not Result).
        // request_async takes a &reqwest::Client (implements AsyncHttpClient).
        let state = state.filter(|s| !s.is_empty()).ok_or_else(|| {
            dog_core::DogError::not_authenticated("Missing OAuth state").into_anyhow()
        })?;
        let verifier = self.verifier.consume(state, ctx).await?;
        let token = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_pkce_verifier(PkceCodeVerifier::new(verifier))
            .request_async(&self.http)
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

        let profile = self
            .http
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
