use std::sync::Arc;

use axum::extract::{OriginalUri, Query};
use axum::http::HeaderMap;
use dog_axum::params::{FromRestParams, RestParams};
use serde_json::Value;

use crate::services::AuthDemoParams;
use dog_auth::AuthenticationService;

use super::providers;

fn tenant_from_headers(headers: &HeaderMap) -> dog_core::tenant::TenantContext {
    headers
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .map(dog_core::tenant::TenantContext::new)
        .unwrap_or_else(|| dog_core::tenant::TenantContext::new("default"))
}

#[derive(serde::Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
}

pub async fn google_login_handler(
    app: Arc<dog_core::DogApp<Value, AuthDemoParams>>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
) -> anyhow::Result<axum::response::Redirect> {
    let tenant = tenant_from_headers(&headers);

    let params = RestParams::from_parts("rest", &headers, Default::default(), "GET", &uri);
    let params: AuthDemoParams = <AuthDemoParams as FromRestParams>::from_rest_params(params);

    let oauth = app.service("oauth")?;
    let res = oauth.custom(tenant, "google_login", None, params).await?;
    let location = res
        .get("location")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("oauth.google_login did not return location"))?;

    Ok(axum::response::Redirect::temporary(location))
}

pub async fn google_callback_handler(
    app: Arc<dog_core::DogApp<Value, AuthDemoParams>>,
    headers: HeaderMap,
    Query(query): Query<OAuthCallbackQuery>,
    OriginalUri(uri): OriginalUri,
) -> anyhow::Result<axum::Json<Value>> {
    let mut q = std::collections::HashMap::new();
    if let Some(code) = query.code.clone() {
        q.insert("code".to_string(), code);
    }
    if let Some(state) = query.state.clone() {
        q.insert("state".to_string(), state);
    }

    let params = RestParams::from_parts("rest", &headers, q, "GET", &uri);
    let params: AuthDemoParams = <AuthDemoParams as FromRestParams>::from_rest_params(params);

    let code = query
        .code
        .ok_or_else(|| anyhow::anyhow!("Missing ?code=..."))?;

    let tenant = tenant_from_headers(&headers);
    let oauth = app.service("oauth")?;
    let res = oauth
        .custom(
            tenant,
            "google_callback",
            Some(serde_json::json!({
                "code": code,
                "state": query.state,
            })),
            params,
        )
        .await?;

    Ok(axum::Json(res))
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

pub async fn google_callback_service_capture_handler(
    Query(query): Query<OAuthCallbackQuery>,
) -> anyhow::Result<axum::Json<Value>> {
    Ok(axum::Json(serde_json::json!({
        "provider": "google_service",
        "code": query.code,
        "state": query.state,
    })))
}
