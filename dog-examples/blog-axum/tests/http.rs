use axum::body::Body;
use axum::http::Request;
use blog_axum::build;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn json_body(res: axum::response::Response) -> Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn health_ok() {
    let ax = build().unwrap();

    let res = ax
        .router
        .oneshot(Request::builder().method("GET").uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status().as_u16(), 200);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(std::str::from_utf8(&bytes).unwrap(), "ok");
}

#[tokio::test]
async fn posts_create_missing_title_is_422() {
    let ax = build().unwrap();

    let res = ax
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .body(Body::from("{\"body\":\"x\"}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status().as_u16(), 422);
    let body = json_body(res).await;
    assert_eq!(body["name"], "Unprocessable");
    assert_eq!(body["code"], 422);
    assert_eq!(body["className"], "unprocessable");
}

#[tokio::test]
async fn posts_create_defaults_published_false_and_sets_request_id() {
    let ax = build().unwrap();

    let res = ax
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .body(Body::from("{\"title\":\"Hello\",\"body\":\"x\"}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status().as_u16(), 200);
    assert!(res.headers().get("x-request-id").is_some());

    let body = json_body(res).await;
    assert_eq!(body["title"], "Hello");
    assert_eq!(body["body"], "x");
    assert_eq!(body["published"], json!(false));
}

#[tokio::test]
async fn posts_find_respects_include_drafts_query_param() {
    let ax = build().unwrap();

    // draft
    let _ = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .body(Body::from("{\"title\":\"Draft\",\"body\":\"x\"}"))
                .unwrap(),
        )
        .await
        .unwrap();

    // published
    let _ = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .body(Body::from(
                    "{\"title\":\"Published\",\"body\":\"x\",\"published\":true}",
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Default: do not include drafts
    let res = ax
        .router
        .clone()
        .oneshot(Request::builder().method("GET").uri("/posts").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let body = json_body(res).await;
    assert_eq!(body.as_array().unwrap().len(), 1);

    // Explicitly include drafts
    let res = ax
        .router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/posts?includeDrafts=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let body = json_body(res).await;
    assert_eq!(body.as_array().unwrap().len(), 2);
}
