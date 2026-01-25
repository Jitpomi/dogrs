mod app;
mod auth;
mod config;
mod hooks;
mod channels;
mod services;

use anyhow::Result;
use axum::extract::{OriginalUri, Query};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use dog_axum::AxumApp;
use dog_axum::params::FromRestParams;
use dog_axum::params::RestParams;
use dog_auth::core::AuthenticationParams;
use dog_auth::AuthenticationService;
use dog_auth_oauth::OAuthService;
use dog_core::HookContext;
use dog_core::ServiceCaller;
use dog_core::ServiceMethodKind;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::services::AuthDemoParams;

fn tenant_from_headers(headers: &HeaderMap) -> dog_core::tenant::TenantContext {
    headers
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .map(dog_core::tenant::TenantContext::new)
        .unwrap_or_else(|| dog_core::tenant::TenantContext::new("default"))
}

async fn google_login_handler(app: Arc<dog_core::DogApp<Value, AuthDemoParams>>) -> Result<Redirect> {
    let url = app
        .get::<String>("oauth.google.authorize_url")
        .ok_or_else(|| anyhow::anyhow!("Missing oauth.google.authorize_url in app config"))?;
    Ok(Redirect::temporary(&url))
}

#[derive(serde::Deserialize)]
struct OAuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

async fn google_callback_handler(
    app: Arc<dog_core::DogApp<Value, AuthDemoParams>>,
    headers: HeaderMap,
    Query(query): Query<OAuthCallbackQuery>,
    OriginalUri(uri): OriginalUri,
) -> Result<axum::Json<Value>> {
    let auth = AuthenticationService::from_app(app.as_ref())
        .ok_or_else(|| anyhow::anyhow!("AuthenticationService missing from app state"))?;

    let mut q = HashMap::new();
    if let Some(code) = query.code.clone() {
        q.insert("code".to_string(), code);
    }
    if let Some(state) = query.state.clone() {
        q.insert("state".to_string(), state);
    }

    let params = RestParams::from_parts("rest", &headers, q, "GET", &uri);
    let params: AuthDemoParams = <AuthDemoParams as FromRestParams>::from_rest_params(params);

    let auth_params = AuthenticationParams {
        payload: None,
        jwt_options: None,
        auth_strategies: None,
        secret: None,
        headers: params.headers.clone(),
    };

    let tenant = tenant_from_headers(&headers);
    let services = ServiceCaller::new(app.as_ref().clone());
    let config = app.as_ref().config_snapshot();
    let mut hook_ctx = HookContext::new(tenant, ServiceMethodKind::Create, params, services, config);

    let code = query
        .code
        .ok_or_else(|| anyhow::anyhow!("Missing ?code=..."))?;

    let mut payload = serde_json::Map::new();
    payload.insert("provider".to_string(), Value::String("google".to_string()));
    payload.insert("code".to_string(), Value::String(code));

    let oauth = OAuthService::new(auth);
    let res = oauth
        .authenticate_callback("oauth", payload, &auth_params, &mut hook_ctx, None)
        .await?;

    Ok(axum::Json(res.auth_result))
}

pub fn build() -> Result<AxumApp<Value, AuthDemoParams>> {
    let ax = app::auth_app()?;

    hooks::global_hooks(ax.app.as_ref());
    channels::configure(ax.app.as_ref())?;

    let svcs = services::configure(ax.app.as_ref())?;

    let app_arc = Arc::clone(&ax.app);
    let oauth_router: Router<()> = Router::new()
        .route(
            "/google/login",
            get({
                let app_arc = Arc::clone(&app_arc);
                move || {
                    let app_arc = Arc::clone(&app_arc);
                    async move {
                        let res: Response = match google_login_handler(app_arc).await {
                            Ok(r) => r.into_response(),
                            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
                        };
                        res
                    }
                }
            }),
        )
        .route(
            "/google/callback",
            get({
                let app_arc = Arc::clone(&app_arc);
                move |headers: HeaderMap, q: Query<OAuthCallbackQuery>, uri: OriginalUri| {
                    let app_arc = Arc::clone(&app_arc);
                    async move {
                        let res: Response = match google_callback_handler(app_arc, headers, q, uri).await {
                            Ok(v) => v.into_response(),
                            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
                        };
                        res
                    }
                }
            }),
        );

    let ax = ax
        .use_service("/messages", svcs.messages)
        .use_service("/users", svcs.users)
        .use_service("/auth", svcs.auth_svc)
        .use_router("/oauth", oauth_router)
        .service("/health", || async { "ok" });

    Ok(ax)
}
