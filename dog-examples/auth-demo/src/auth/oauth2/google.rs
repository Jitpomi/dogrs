
use std::sync::Arc;

use crate::services::AuthDemoParams;
use dog_auth::AuthenticationService;
use dog_auth_oauth::{OAuthEntityResolver, OAuthProvider, OAuthStrategy, OAuthStrategyOptions};
use dog_core::HookContext;
use oauth2::basic::BasicClient;
use oauth2::reqwest::async_http_client;
use oauth2::{AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, RedirectUrl, Scope, TokenResponse, TokenUrl};
use serde_json::{json, Value};

struct GoogleOAuthProvider {
    client: BasicClient,
}

impl GoogleOAuthProvider {
    fn from_app(auth: &AuthenticationService<AuthDemoParams>) -> anyhow::Result<Self> {
        let app = auth.base.app();

        let client_id = app
            .get::<String>("oauth.google.client_id")
            .ok_or_else(|| anyhow::anyhow!("Missing oauth.google.client_id"))?;

        let client_secret = app
            .get::<String>("oauth.google.client_secret")
            .ok_or_else(|| anyhow::anyhow!("Missing oauth.google.client_secret"))?;

        let redirect_url = app
            .get::<String>("oauth.google.redirect_uri")
            .ok_or_else(|| anyhow::anyhow!("Missing oauth.google.redirect_uri"))?;

        let auth_url = AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())?;
        let token_url = TokenUrl::new("https://oauth2.googleapis.com/token".to_string())?;

        let client = BasicClient::new(
            ClientId::new(client_id),
            Some(ClientSecret::new(client_secret)),
            auth_url,
            Some(token_url),
        )
        .set_redirect_uri(RedirectUrl::new(redirect_url)?);

        Ok(Self { client })
    }

    fn authorize_url(&self) -> String {
        let (url, _csrf) = self
            .client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new("openid".to_string()))
            .add_scope(Scope::new("email".to_string()))
            .add_scope(Scope::new("profile".to_string()))
            .url();

        url.to_string()
    }
}

#[async_trait::async_trait]
impl OAuthProvider<AuthDemoParams> for GoogleOAuthProvider {
    fn name(&self) -> &str {
        "google"
    }

    async fn exchange_code(&self, code: &str, _ctx: &mut HookContext<Value, AuthDemoParams>) -> anyhow::Result<String> {
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
        _ctx: &mut HookContext<Value, AuthDemoParams>,
    ) -> anyhow::Result<Option<Value>> {
        let client = reqwest::Client::new();
        let profile = client
            .get("https://openidconnect.googleapis.com/v1/userinfo")
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        Ok(Some(profile))
    }
}

struct GoogleEntityResolver;

#[async_trait::async_trait]
impl OAuthEntityResolver<AuthDemoParams> for GoogleEntityResolver {
    async fn resolve_entity(
        &self,
        provider: &str,
        profile: &Value,
        ctx: &mut HookContext<Value, AuthDemoParams>,
    ) -> anyhow::Result<Option<Value>> {
        let _ = provider;
        let users = ctx.services.service::<Value, AuthDemoParams>("users")?;

        let google_id = profile.get("sub").and_then(|v| v.as_str()).unwrap_or("");
        if google_id.trim().is_empty() {
            return Ok(None);
        }

        let all = users.find(&ctx.tenant, ctx.params.clone()).await?;
        if let Some(existing) = all
            .into_iter()
            .find(|u| u.get("googleId").and_then(|v| v.as_str()) == Some(google_id))
        {
            return Ok(Some(existing));
        }

        let username = profile
            .get("email")
            .and_then(|v| v.as_str())
            .or_else(|| profile.get("name").and_then(|v| v.as_str()))
            .unwrap_or("google-user")
            .to_string();

        let random_pw = uuid::Uuid::new_v4().to_string();
        let created = users
            .create(
                &ctx.tenant,
                json!({
                    "username": username,
                    "password": random_pw,
                    "googleId": google_id,
                }),
                ctx.params.clone(),
            )
            .await?;

        Ok(Some(created))
    }
}

pub fn register_google_oauth(auth: Arc<AuthenticationService<AuthDemoParams>>) -> anyhow::Result<String> {
    let provider = Arc::new(GoogleOAuthProvider::from_app(auth.as_ref())?);
    let authorize_url = provider.authorize_url();

    let mut opts: OAuthStrategyOptions<AuthDemoParams> = OAuthStrategyOptions::default();
    opts.default_provider = Some("google".to_string());
    opts.providers.insert("google".to_string(), provider);
    opts.entity_resolver = Some(Arc::new(GoogleEntityResolver));

    let strategy = OAuthStrategy::new(&auth.base).with_options(opts);
    auth.register_strategy("oauth", Arc::new(strategy));
    Ok(authorize_url)
}

