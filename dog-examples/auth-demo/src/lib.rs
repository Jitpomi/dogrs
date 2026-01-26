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
use axum::response::{IntoResponse, Response};
use dog_axum::AxumApp;
use serde_json::Value;
use std::sync::Arc;

use crate::services::AuthDemoParams;

pub fn build() -> Result<AxumApp<Value, AuthDemoParams>> {
    let ax = app::auth_app()?;

    hooks::global_hooks(ax.app.as_ref());
    channels::configure(ax.app.as_ref())?;

    let svcs = services::configure(ax.app.as_ref())?;

    let app_arc = Arc::clone(&ax.app);

    let ax = ax
        .use_service("/messages", svcs.messages)
        .use_service("/users", svcs.users)
        .use_service("/auth", svcs.auth_svc)
        .use_service("/oauth", svcs.oauth)
        .service(
            "/oauth/google/login",
            {
                let app_arc = Arc::clone(&app_arc);
                move |headers: HeaderMap, uri: OriginalUri| {
                    let app_arc = Arc::clone(&app_arc);
                    async move {
                        let res: Response = match crate::auth::oauth2::google::google_login_handler(
                            app_arc,
                            headers,
                            uri,
                        )
                        .await
                        {
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
                        let res: Response = match crate::auth::oauth2::google::google_login_service_handler(app_arc).await {
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
                move |headers: HeaderMap, Query(query): Query<crate::auth::oauth2::google::OAuthCallbackQuery>, uri: OriginalUri| {
                    let app_arc = Arc::clone(&app_arc);
                    async move {
                        let res: Response = match crate::auth::oauth2::google::google_callback_handler(
                            app_arc,
                            headers,
                            Query(query),
                            uri,
                        )
                        .await
                        {
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
                move |Query(query): Query<crate::auth::oauth2::google::OAuthCallbackQuery>| async move {
                    let res: Response = match crate::auth::oauth2::google::google_callback_service_capture_handler(Query(query)).await {
                        Ok(r) => r.into_response(),
                        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
                    };
                    res
                }
            },
        )
        .service("/health", || async { "ok" });

    Ok(ax)
}
