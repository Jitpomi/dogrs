use std::sync::Arc;

use crate::services::AuthDemoParams;

use dog_auth_oauth::{
    OAuth2AuthorizationCodeProvider, OAuthEntityResolver, OAuthStrategy, OAuthStrategyOptions,
};
use dog_core::HookContext;
use serde_json::{json, Value};

type GoogleOAuthProvider = OAuth2AuthorizationCodeProvider<AuthDemoParams>;

fn google_provider_with_redirect(
    config: &dog_core::DogConfigSnapshot,
    name: &'static str,
    redirect_uri: &str,
) -> anyhow::Result<GoogleOAuthProvider> {
    let client_id = config
        .get_string("oauth.google.client_id")
        .ok_or_else(|| anyhow::anyhow!("Missing oauth.google.client_id"))?;

    let client_secret = config
        .get_string("oauth.google.client_secret")
        .ok_or_else(|| anyhow::anyhow!("Missing oauth.google.client_secret"))?;

    GoogleOAuthProvider::new(
        dog_auth_oauth::oauth2_client::OAuth2ClientConfig {
            name: name.to_string(),
            client_id,
            client_secret,
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_url: "https://oauth2.googleapis.com/token".to_string(),
            redirect_uri: redirect_uri.to_string(),
            scopes: vec![
                "openid".to_string(),
                "email".to_string(),
                "profile".to_string(),
            ],
            userinfo_url: Some("https://openidconnect.googleapis.com/v1/userinfo".to_string()),
        },
        Arc::new(BrowserCallbackVerifier {
            redirect_uri: redirect_uri.to_string(),
        }),
    )
}

struct PendingLogin {
    expires: std::time::Instant,
    verifier: String,
    redirect_uri: String,
}
fn pending_logins() -> &'static std::sync::Mutex<std::collections::HashMap<String, PendingLogin>> {
    static PENDING: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, PendingLogin>>,
    > = std::sync::OnceLock::new();
    PENDING.get_or_init(Default::default)
}
struct BrowserCallbackVerifier {
    redirect_uri: String,
}
#[async_trait::async_trait]
impl dog_auth_oauth::OAuthCallbackVerifier<AuthDemoParams> for BrowserCallbackVerifier {
    async fn consume(
        &self,
        state: &str,
        ctx: &mut HookContext<Value, AuthDemoParams>,
    ) -> anyhow::Result<String> {
        let cookies = ctx
            .params
            .headers
            .get("cookie")
            .map(String::as_str)
            .unwrap_or("");
        let cookie = cookies
            .split(';')
            .filter_map(|s| s.trim().split_once('='))
            .find(|(name, _)| *name == "dogrs_oauth_state")
            .map(|(_, value)| value);
        if cookie != Some(state) {
            return Err(
                dog_core::DogError::not_authenticated("OAuth browser state mismatch").into_anyhow(),
            );
        }
        let mut pending = pending_logins()
            .lock()
            .map_err(|_| anyhow::anyhow!("OAuth state lock failed"))?;
        let login = pending.remove(state).ok_or_else(|| {
            dog_core::DogError::not_authenticated("Unknown or consumed OAuth state").into_anyhow()
        })?;
        if login.expires <= std::time::Instant::now() || login.redirect_uri != self.redirect_uri {
            return Err(
                dog_core::DogError::not_authenticated("Expired or mismatched OAuth state")
                    .into_anyhow(),
            );
        }
        Ok(login.verifier)
    }
}

#[derive(serde::Serialize)]
pub struct Login {
    pub location: String,
    pub state: String,
    pub secure_cookie: bool,
}
/// Demo-only bounded state store. Multi-instance deployments need shared session storage.
pub fn authorize_url_for_redirect(
    config: &dog_core::DogConfigSnapshot,
    redirect_uri: &str,
) -> anyhow::Result<Login> {
    let authorization =
        google_provider_with_redirect(config, "google", redirect_uri)?.authorize_url();
    let mut pending = pending_logins()
        .lock()
        .map_err(|_| anyhow::anyhow!("OAuth state lock failed"))?;
    let now = std::time::Instant::now();
    pending.retain(|_, login| login.expires > now);
    if pending.len() >= 1024 {
        return Err(anyhow::anyhow!("Too many pending OAuth logins"));
    }
    pending.insert(
        authorization.state.clone(),
        PendingLogin {
            expires: now + std::time::Duration::from_secs(600),
            verifier: authorization.code_verifier,
            redirect_uri: redirect_uri.to_string(),
        },
    );
    Ok(Login {
        location: authorization.url,
        state: authorization.state,
        secure_cookie: redirect_uri.starts_with("https://"),
    })
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
        let users = ctx.services.service("users")?;

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
    builder: &mut dog_core::DogAppBuilder<Value, AuthDemoParams>,
    auth: &mut dog_auth::core::AuthenticationBuilder<AuthDemoParams>,
) -> anyhow::Result<()> {
    let config = builder.config_snapshot();
    let redirect_uri = config
        .get_string("oauth.google.redirect_uri")
        .ok_or_else(|| anyhow::anyhow!("Missing oauth.google.redirect_uri"))?;

    let provider = Arc::new(google_provider_with_redirect(
        &config,
        "google",
        &redirect_uri,
    )?);

    let redirect_service = if redirect_uri.ends_with("/oauth/google/callback") {
        format!("{redirect_uri}/service")
    } else {
        return Err(anyhow::anyhow!(
            "oauth.google.redirect_uri must end with /oauth/google/callback to derive /service variant"
        ));
    };
    let provider_service = Arc::new(google_provider_with_redirect(
        &config,
        "google_service",
        &redirect_service,
    )?);

    let mut opts = OAuthStrategyOptions {
        default_provider: Some("google".to_string()),
        ..Default::default()
    };
    opts.providers.insert("google".to_string(), provider);
    opts.providers
        .insert("google_service".to_string(), provider_service);
    opts.entity_resolver = Some(Arc::new(GoogleEntityResolver));

    let strategy = OAuthStrategy::new().with_options(opts);
    auth.register("oauth", Arc::new(strategy));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dog_auth_oauth::OAuthCallbackVerifier;
    #[tokio::test]
    async fn callback_state_is_bound_to_browser_and_consumed_once() {
        let state = uuid::Uuid::new_v4().to_string();
        pending_logins().lock().unwrap().insert(
            state.clone(),
            PendingLogin {
                expires: std::time::Instant::now() + std::time::Duration::from_secs(60),
                verifier: "test-verifier".into(),
                redirect_uri: "https://example.invalid/callback".into(),
            },
        );
        let app = dog_core::DogAppBuilder::<Value, AuthDemoParams>::new().build();
        let mut ctx = HookContext::new(
            dog_core::TenantContext::new("test"),
            dog_core::ServiceMethodKind::Create,
            AuthDemoParams::default(),
            dog_core::ServiceCaller::new(app.clone()),
            app.config_snapshot(),
        );
        let guard = BrowserCallbackVerifier {
            redirect_uri: "https://example.invalid/callback".into(),
        };
        assert!(guard.consume(&state, &mut ctx).await.is_err());
        ctx.params
            .headers
            .insert("cookie".into(), format!("dogrs_oauth_state={state}"));
        assert_eq!(
            guard.consume(&state, &mut ctx).await.unwrap(),
            "test-verifier"
        );
        assert!(guard.consume(&state, &mut ctx).await.is_err());
    }
}
