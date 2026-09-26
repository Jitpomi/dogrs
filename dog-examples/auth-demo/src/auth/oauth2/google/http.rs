use super::providers;
use crate::services::AuthDemoParams;
use dog_core::{DogApp, DogMethod, DogParams, DogRequest, DogTransportKind, TenantContext};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, serde::Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
}

async fn login(app: &DogApp<Value, AuthDemoParams>, service: bool) -> actix_web::HttpResponse {
    let config = app.config_snapshot();
    let Some(mut redirect) = config.get_string("oauth.google.redirect_uri") else {
        return actix_web::HttpResponse::InternalServerError().finish();
    };
    if service {
        redirect.push_str("/service");
    }
    match providers::authorize_url_for_redirect(&config, &redirect) {
        Ok(login) => actix_web::HttpResponse::TemporaryRedirect()
            .insert_header((actix_web::http::header::LOCATION, login.location))
            .cookie(
                actix_web::cookie::Cookie::build("dogrs_oauth_state", login.state)
                    .http_only(true)
                    .secure(login.secure_cookie)
                    .same_site(actix_web::cookie::SameSite::Lax)
                    .path("/oauth/google")
                    .max_age(actix_web::cookie::time::Duration::minutes(10))
                    .finish(),
            )
            .finish(),
        Err(err) => {
            tracing::error!(%err, "OAuth login failed");
            actix_web::HttpResponse::ServiceUnavailable().finish()
        }
    }
}
async fn callback(
    app: &DogApp<Value, AuthDemoParams>,
    query: OAuthCallbackQuery,
    http: actix_web::HttpRequest,
    service: bool,
) -> actix_web::HttpResponse {
    let headers: std::collections::HashMap<String, String> = http
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.to_string(), v.to_string()))
        })
        .collect();
    let mut params = DogParams::new();
    params
        .inner
        .insert("headers".into(), serde_json::json!(headers));
    params.inner.insert(
        "inner".into(),
        serde_json::to_value(crate::services::types::RestParams::default())
            .expect("RestParams serialization"),
    );
    let req = DogRequest {
        request_id: None,
        transport: DogTransportKind::Http,
        service: "oauth".into(),
        method: DogMethod::Custom("google_callback".into()),
        id: None,
        tenant: TenantContext::new("default"),
        params,
        payload: Some(
            serde_json::json!({"provider": if service { "google_service" } else { "google" }, "code": query.code, "state": query.state}),
        ),
        metadata: Default::default(),
    };
    match app.handle(req).await {
        Ok(res) => actix_web::HttpResponse::Ok().json(res.payload),
        Err(err) => actix_web::HttpResponse::build(
            actix_web::http::StatusCode::from_u16(err.code())
                .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR),
        )
        .body(err.sanitize_for_client().message),
    }
}
pub fn configure(cfg: &mut actix_web::web::ServiceConfig, app: Arc<DogApp<Value, AuthDemoParams>>) {
    for (path, service) in [
        ("/oauth/google/login", false),
        ("/oauth/google/login/service", true),
    ] {
        let app = app.clone();
        cfg.service(
            actix_web::web::resource(path).route(actix_web::web::get().to(move || {
                let app = app.clone();
                async move { login(&app, service).await }
            })),
        );
    }
    for (path, service) in [
        ("/oauth/google/callback", false),
        ("/oauth/google/callback/service", true),
    ] {
        let app = app.clone();
        cfg.service(
            actix_web::web::resource(path).route(actix_web::web::get().to(
                move |query: actix_web::web::Query<OAuthCallbackQuery>,
                      request: actix_web::HttpRequest| {
                    let app = app.clone();
                    async move { callback(&app, query.into_inner(), request, service).await }
                },
            )),
        );
    }
}
