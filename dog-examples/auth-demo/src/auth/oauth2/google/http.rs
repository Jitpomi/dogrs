use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use dog_axum::oauth;
use dog_axum::AxumApp;
use serde_json::Value;

use crate::services::AuthDemoParams;
use dog_auth::AuthenticationService;

use super::providers;

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
}

fn service_redirect_uri(app: &dog_core::DogApp<Value, AuthDemoParams>) -> anyhow::Result<String> {
    let base = app
        .get::<String>("oauth.google.redirect_uri")
        .ok_or_else(|| anyhow::anyhow!("Missing oauth.google.redirect_uri"))?;

    if base.ends_with("/oauth/google/callback") {
        return Ok(format!("{base}/service"));
    }

    Err(anyhow::anyhow!(
        "oauth.google.redirect_uri must end with /oauth/google/callback to derive /service variant"
    ))
}

pub async fn google_login_service_handler(
    app: Arc<dog_core::DogApp<Value, AuthDemoParams>>,
) -> anyhow::Result<axum::response::Redirect> {
    let auth = AuthenticationService::from_app(app.as_ref())
        .ok_or_else(|| anyhow::anyhow!("AuthenticationService missing from app state"))?;

    let redirect_uri = service_redirect_uri(app.as_ref())?;
    let location = providers::authorize_url_for_redirect(auth.as_ref(), &redirect_uri)?;

    Ok(axum::response::Redirect::temporary(&location))
}

pub fn mount(ax: AxumApp<Value, AuthDemoParams>) -> AxumApp<Value, AuthDemoParams> {
    let routes = oauth::OAuthRoutes::new(
        "/oauth/google/login",
        "/oauth/google/callback",
        "oauth",
        "google_login",
        "google_callback",
        |q: &OAuthCallbackQuery| {
            serde_json::json!({
                "code": q.code,
                "state": q.state,
            })
        },
    )
    .with_capture("/oauth/google/callback/service", "google_service")
    .with_http_method("GET");

    let ax = oauth::mount_oauth_routes::<AuthDemoParams, OAuthCallbackQuery, _>(ax, routes);

    let app_arc = Arc::clone(&ax.app);
    ax.service(
        "/oauth/google/login/service",
        {
            move || {
                let app_arc = Arc::clone(&app_arc);
                async move {
                    let res: Response = match google_login_service_handler(app_arc).await {
                        Ok(r) => r.into_response(),
                        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
                    };
                    res
                }
            }
        },
    )
}
