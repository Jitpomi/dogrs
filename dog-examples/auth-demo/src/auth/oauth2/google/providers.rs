use std::sync::Arc;

use crate::services::AuthDemoParams;
use dog_auth::AuthenticationService;
use dog_auth_oauth::{OAuth2AuthorizationCodeProvider, OAuthEntityResolver, OAuthStrategy, OAuthStrategyOptions};
use dog_core::HookContext;
use serde_json::{json, Value};

type GoogleOAuthProvider = OAuth2AuthorizationCodeProvider<AuthDemoParams>;

fn google_provider_with_redirect(
    auth: &AuthenticationService<AuthDemoParams>,
    name: &'static str,
    redirect_uri: &str,
) -> anyhow::Result<GoogleOAuthProvider> {
    let app = auth.base.app();

    let client_id = app
        .get::<String>("oauth.google.client_id")
        .ok_or_else(|| anyhow::anyhow!("Missing oauth.google.client_id"))?;

    let client_secret = app
        .get::<String>("oauth.google.client_secret")
        .ok_or_else(|| anyhow::anyhow!("Missing oauth.google.client_secret"))?;

    Ok(GoogleOAuthProvider::new(
        name,
        client_id,
        client_secret,
        "https://accounts.google.com/o/oauth2/v2/auth",
        "https://oauth2.googleapis.com/token",
        redirect_uri.to_string(),
        vec!["openid".to_string(), "email".to_string(), "profile".to_string()],
        Some("https://openidconnect.googleapis.com/v1/userinfo".to_string()),
    )?)
}

pub fn authorize_url_for_redirect(
    auth: &AuthenticationService<AuthDemoParams>,
    redirect_uri: &str,
) -> anyhow::Result<String> {
    Ok(google_provider_with_redirect(auth, "google", redirect_uri)?.authorize_url())
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

pub fn register_google_oauth(
    auth: Arc<AuthenticationService<AuthDemoParams>>,
) -> anyhow::Result<String> {
    let redirect_uri = auth
        .base
        .app()
        .get::<String>("oauth.google.redirect_uri")
        .ok_or_else(|| anyhow::anyhow!("Missing oauth.google.redirect_uri"))?;

    let provider = Arc::new(google_provider_with_redirect(auth.as_ref(), "google", &redirect_uri)?);
    let authorize_url = provider.authorize_url();

    let redirect_service = if redirect_uri.ends_with("/oauth/google/callback") {
        format!("{redirect_uri}/service")
    } else {
        return Err(anyhow::anyhow!(
            "oauth.google.redirect_uri must end with /oauth/google/callback to derive /service variant"
        ));
    };
    let provider_service = Arc::new(google_provider_with_redirect(
        auth.as_ref(),
        "google_service",
        &redirect_service,
    )?);

    let mut opts: OAuthStrategyOptions<AuthDemoParams> = OAuthStrategyOptions::default();
    opts.default_provider = Some("google".to_string());
    opts.providers.insert("google".to_string(), provider);
    opts.providers
        .insert("google_service".to_string(), provider_service);
    opts.entity_resolver = Some(Arc::new(GoogleEntityResolver));

    let strategy = OAuthStrategy::new(&auth.base).with_options(opts);
    auth.register_strategy("oauth", Arc::new(strategy));
    Ok(authorize_url)
}
