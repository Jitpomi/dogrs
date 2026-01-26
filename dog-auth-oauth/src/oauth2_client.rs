use std::marker::PhantomData;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::HookContext;
use oauth2::basic::BasicClient;
use oauth2::reqwest::async_http_client;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, RedirectUrl, Scope, TokenResponse,
    TokenUrl,
};
use serde_json::Value;

use crate::strategy::OAuthProvider;

pub struct OAuth2AuthorizationCodeProvider<P>
where
    P: Clone + Send + Sync + 'static,
{
    name: String,
    client: BasicClient,
    scopes: Vec<String>,
    userinfo_url: Option<String>,
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
        let client = BasicClient::new(
            ClientId::new(client_id.into()),
            Some(ClientSecret::new(client_secret.into())),
            AuthUrl::new(auth_url.into())?,
            Some(TokenUrl::new(token_url.into())?),
        )
        .set_redirect_uri(RedirectUrl::new(redirect_uri.into())?);

        Ok(Self {
            name: name.into(),
            client,
            scopes,
            userinfo_url,
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
        let token = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .request_async(async_http_client)
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
