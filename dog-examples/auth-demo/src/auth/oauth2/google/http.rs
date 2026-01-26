use std::sync::Arc;

use axum::extract::{OriginalUri, Query};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use dog_axum::params::{FromRestParams, RestParams};
use dog_axum::AxumApp;
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

pub fn mount(mut ax: AxumApp<Value, AuthDemoParams>) -> AxumApp<Value, AuthDemoParams> {
    let app_arc = Arc::clone(&ax.app);

    ax = ax
        .service(
            "/oauth/google/login",
            {
                let app_arc = Arc::clone(&app_arc);
                move |headers: HeaderMap, uri: OriginalUri| {
                    let app_arc = Arc::clone(&app_arc);
                    async move {
                        let res: Response = match google_login_handler(app_arc, headers, uri).await {
                            Ok(r) => r.into_response(),
                            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
                        };
                        res
                    }
                }
            },
        )
        .service(
            "/oauth/google/login/service",
            {
                let app_arc = Arc::clone(&app_arc);
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
        .service(
            "/oauth/google/callback",
            {
                let app_arc = Arc::clone(&app_arc);
                move |headers: HeaderMap, Query(query): Query<OAuthCallbackQuery>, uri: OriginalUri| {
                    let app_arc = Arc::clone(&app_arc);
                    async move {
                        let res: Response = match google_callback_handler(app_arc, headers, Query(query), uri).await {
                            Ok(r) => r.into_response(),
                            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
                        };
                        res
                    }
                }
            },
        )
        .service(
            "/oauth/google/callback/service",
            {
                move |Query(query): Query<OAuthCallbackQuery>| async move {
                    let res: Response = match google_callback_service_capture_handler(Query(query)).await {
                        Ok(r) => r.into_response(),
                        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
                    };
                    res
                }
            },
        );

    ax
}
