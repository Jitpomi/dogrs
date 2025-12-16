use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use axum::http::HeaderValue;
use dog_axum::axum;
use dog_core::errors::DogError;
use dog_core::tenant::TenantContext;
use dog_core::{DogApp, DogService, ServiceCapabilities, ServiceMethodKind};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

struct UnprocessableOnCreate;

#[async_trait::async_trait]
impl DogService<Value, ()> for UnprocessableOnCreate {
    fn capabilities(&self) -> ServiceCapabilities {
        ServiceCapabilities::from_methods(vec![ServiceMethodKind::Create])
    }

    async fn create(&self, _ctx: &TenantContext, _data: Value, _params: ()) -> anyhow::Result<Value> {
        Err(DogError::unprocessable("Invalid")
            .with_errors(json!({"title": ["required"]}))
            .into_anyhow())
    }
}

struct BoomOnCreate;

#[async_trait::async_trait]
impl DogService<Value, ()> for BoomOnCreate {
    fn capabilities(&self) -> ServiceCapabilities {
        ServiceCapabilities::from_methods(vec![ServiceMethodKind::Create])
    }

    async fn create(&self, _ctx: &TenantContext, _data: Value, _params: ()) -> anyhow::Result<Value> {
        Err(anyhow::anyhow!("boom"))
    }
}

async fn json_body(res: axum::response::Response) -> Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn malformed_json_returns_dogerror_bad_request() {
    let app: DogApp<Value, ()> = DogApp::new();
    let ax = axum(app).use_service("/posts", Arc::new(BoomOnCreate));

    let res = ax
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .body(Body::from("{\"title\":\"x\""))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status().as_u16(), 400);
    assert!(res.headers().get("x-request-id").is_some());
    let body = json_body(res).await;
    assert_eq!(body["name"], "BadRequest");
    assert_eq!(body["code"], 400);
    assert_eq!(body["className"], "bad-request");
    assert!(body.get("errors").is_some());
}

#[tokio::test]
async fn request_id_is_preserved_when_provided() {
    let app: DogApp<Value, ()> = DogApp::new();
    let ax = axum(app).use_service("/posts", Arc::new(BoomOnCreate));

    let provided = HeaderValue::from_static("req-test-123");
    let res = ax
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .header("x-request-id", provided.clone())
                .body(Body::from("{\"title\":\"ok\",\"body\":\"x\"}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.headers().get("x-request-id").unwrap(), &provided);
}

#[tokio::test]
async fn dogerror_unprocessable_preserves_422_and_shape() {
    let app: DogApp<Value, ()> = DogApp::new();
    let ax = axum(app).use_service("/posts", Arc::new(UnprocessableOnCreate));

    let res = ax
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .body(Body::from("{\"title\":\"ok\",\"body\":\"x\"}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status().as_u16(), 422);
    let body = json_body(res).await;
    assert_eq!(body["name"], "Unprocessable");
    assert_eq!(body["code"], 422);
    assert_eq!(body["className"], "unprocessable");
    assert_eq!(body["errors"], json!({"title": ["required"]}));
}

#[tokio::test]
async fn non_dogerror_maps_to_generalerror_shape() {
    let app: DogApp<Value, ()> = DogApp::new();
    let ax = axum(app).use_service("/posts", Arc::new(BoomOnCreate));

    let res = ax
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .body(Body::from("{\"title\":\"ok\",\"body\":\"x\"}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status().as_u16(), 500);
    let body = json_body(res).await;
    assert_eq!(body["name"], "GeneralError");
    assert_eq!(body["code"], 500);
    assert_eq!(body["className"], "general-error");
    assert!(body["message"].as_str().unwrap().contains("boom"));
}
