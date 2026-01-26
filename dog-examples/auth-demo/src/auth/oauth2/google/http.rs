use std::sync::Arc;

use axum::extract::{OriginalUri, Query};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use dog_axum::rest;
use dog_axum::AxumApp;
use serde_json::Value;

use crate::services::AuthDemoParams;
use dog_auth::AuthenticationService;

use super::providers;

#[derive(serde::Deserialize, serde::Serialize)]
pub struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
}

pub async fn google_login_handler(
    app: Arc<dog_core::DogApp<Value, AuthDemoParams>>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
) -> anyhow::Result<axum::response::Redirect> {
    let res = rest::call_custom_redirect(
        app.as_ref(),
        "oauth",
        "google_login",
        &headers,
        Default::default(),
        "GET",
        &uri,
        None,
        "location",
    )
    .await
    .map_err(|e| e.0)?;

    Ok(res)
}

pub async fn google_callback_handler(
    app: Arc<dog_core::DogApp<Value, AuthDemoParams>>,
    headers: HeaderMap,
    Query(query): Query<OAuthCallbackQuery>,
    OriginalUri(uri): OriginalUri,
) -> anyhow::Result<axum::Json<Value>> {
    let q = rest::query_to_map(&query);

    let code = query
        .code
        .ok_or_else(|| anyhow::anyhow!("Missing ?code=..."))?;

    let res = rest::call_custom_json(
        app.as_ref(),
        "oauth",
        "google_callback",
        &headers,
        q,
        "GET",
        &uri,
        Some(serde_json::json!({
            "code": code,
            "state": query.state,
        })),
    )
    .await
    .map_err(|e| e.0)?;

    Ok(res)
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
    Ok(rest::oauth_callback_capture("google_service", &query))
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
