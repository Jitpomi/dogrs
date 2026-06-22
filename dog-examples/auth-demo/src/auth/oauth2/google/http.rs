use std::sync::Arc;
use dog_core::{DogApp, DogRequest, DogTransportKind, DogMethod, DogParams, TenantContext};
use serde_json::Value;
use crate::services::AuthDemoParams;
use super::providers;

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
}

fn service_redirect_uri(app: &DogApp<Value, AuthDemoParams>) -> anyhow::Result<String> {
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
    app: &DogApp<Value, AuthDemoParams>,
) -> anyhow::Result<String> {
    let config = app.config_snapshot();
    let redirect_uri = service_redirect_uri(app)?;
    let location = providers::authorize_url_for_redirect(&config, &redirect_uri)?;

    Ok(location)
}

pub fn configure(cfg: &mut actix_web::web::ServiceConfig, app: Arc<DogApp<Value, AuthDemoParams>>) {
    let app_clone = Arc::clone(&app);
    let app_clone2 = Arc::clone(&app);
    let app_clone3 = Arc::clone(&app);

    cfg.service(
        actix_web::web::resource("/oauth/google/login")
            .route(actix_web::web::get().to(move || {
                let app = Arc::clone(&app_clone);
                async move {
                    let req = DogRequest {
                        request_id: Some(uuid::Uuid::new_v4().to_string()),
                        transport: DogTransportKind::Http,
                        service: "oauth".to_string(),
                        method: DogMethod::Custom("google_login".to_string()),
                        id: None,
                        tenant: TenantContext::new("default"),
                        params: DogParams::new(),
                        payload: None,
                        metadata: std::collections::HashMap::new(),
                    };

                    match app.handle(req).await {
                        Ok(res) => {
                            if let Some(loc) = res.payload.and_then(|p| p.get("location").and_then(|v| v.as_str().map(|s| s.to_string()))) {
                                actix_web::HttpResponse::TemporaryRedirect()
                                    .insert_header((actix_web::http::header::LOCATION, loc))
                                    .finish()
                            } else {
                                actix_web::HttpResponse::InternalServerError().body("Missing redirect location in oauth response")
                            }
                        }
                        Err(err) => {
                            actix_web::HttpResponse::build(
                                actix_web::http::StatusCode::from_u16(err.code())
                                    .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR)
                            ).body(err.message)
                        }
                    }
                }
            }))
    )
    .service(
        actix_web::web::resource("/oauth/google/callback")
            .route(actix_web::web::get().to(move |query: actix_web::web::Query<OAuthCallbackQuery>| {
                let app = Arc::clone(&app_clone2);
                async move {
                    let payload = serde_json::json!({
                        "code": query.code,
                        "state": query.state,
                    });

                    let req = DogRequest {
                        request_id: Some(uuid::Uuid::new_v4().to_string()),
                        transport: DogTransportKind::Http,
                        service: "oauth".to_string(),
                        method: DogMethod::Custom("google_callback".to_string()),
                        id: None,
                        tenant: TenantContext::new("default"),
                        params: DogParams::new(),
                        payload: Some(payload),
                        metadata: std::collections::HashMap::new(),
                    };

                    match app.handle(req).await {
                        Ok(res) => {
                            actix_web::HttpResponse::Ok().json(res.payload.unwrap_or(Value::Null))
                        }
                        Err(err) => {
                            actix_web::HttpResponse::build(
                                actix_web::http::StatusCode::from_u16(err.code())
                                    .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR)
                            ).body(err.message)
                        }
                    }
                }
            }))
    )
    .service(
        actix_web::web::resource("/oauth/google/callback/service")
            .route(actix_web::web::get().to(|query: actix_web::web::Query<OAuthCallbackQuery>| async move {
                actix_web::HttpResponse::Ok().json(serde_json::json!({
                    "provider": "google_service",
                    "code": query.code,
                    "state": query.state,
                }))
            }))
    )
    .service(
        actix_web::web::resource("/oauth/google/login/service")
            .route(actix_web::web::get().to(move || {
                let app = Arc::clone(&app_clone3);
                async move {
                    match google_login_service_handler(&app).await {
                        Ok(loc) => actix_web::HttpResponse::TemporaryRedirect()
                            .insert_header((actix_web::http::header::LOCATION, loc))
                            .finish(),
                        Err(e) => actix_web::HttpResponse::InternalServerError().body(e.to_string()),
                    }
                }
            }))
    );
}
