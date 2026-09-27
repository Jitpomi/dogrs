use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware,
    response::Response,
    routing::post,
    Router,
};
use dog_axum::middlewares::multipart::{MultipartConfig, MultipartToJson};
use tower::ServiceExt;

#[tokio::test]
async fn multipart_preserves_request_parts() {
    let router = Router::new()
        .route(
            "/upload",
            post(|req: Request<Body>| async move {
                assert_eq!(req.method(), "POST");
                assert_eq!(req.uri().path(), "/upload");
                assert_eq!(req.extensions().get::<u32>(), Some(&42));
                StatusCode::OK
            }),
        )
        .layer(MultipartToJson::new());
    let mut req = multipart_request();
    req.extensions_mut().insert(42u32);
    router.oneshot(req).await.unwrap();
}
fn multipart_request() -> Request<Body> {
    Request::builder().method("POST").uri("/upload")
        .header("content-type", "multipart/form-data; boundary=test")
        .body(Body::from("--test\r\nContent-Disposition: form-data; name=\"message\"\r\n\r\nhello\r\n--test--\r\n")).unwrap()
}
#[tokio::test]
async fn multipart_honors_total_limit() {
    let router = Router::new()
        .route("/upload", post(|| async { StatusCode::OK }))
        .layer(MultipartToJson::with_config(
            MultipartConfig::new().max_total_size(1),
        ));
    let res = router.oneshot(multipart_request()).await.unwrap();
    assert!(
        res.status().is_client_error(),
        "oversized request accepted: {}",
        res.status()
    );
}
struct Empty;
#[async_trait::async_trait]
impl dog_core::DogService<serde_json::Value, ()> for Empty {}
async fn reject(_: Request<Body>, _: middleware::Next) -> Response {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .body(Body::empty())
        .unwrap()
}
#[tokio::test]
async fn cloned_app_preserves_pending_auth_middleware() {
    let app: dog_core::DogApp<serde_json::Value, ()> = Default::default();
    let configured = dog_axum::axum(app).use_middleware(middleware::from_fn(reject));
    let app = configured
        .clone()
        .use_service("/items", std::sync::Arc::new(Empty));
    let res = app
        .router
        .oneshot(
            Request::builder()
                .uri("/items")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn service_specific_layer_keeps_shared_auth_middleware() {
    let app: dog_core::DogApp<serde_json::Value, ()> = Default::default();
    let configured = dog_axum::axum(app).use_middleware(middleware::from_fn(reject));
    let app = configured.use_service_with(
        "/items",
        std::sync::Arc::new(Empty),
        tower::layer::util::Identity::new(),
    );
    let res = app
        .router
        .oneshot(
            Request::builder()
                .uri("/items")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

struct Echo;
#[async_trait::async_trait]
impl dog_core::DogService<serde_json::Value, dog_transport::http::RestParams> for Echo {
    async fn get(
        &self,
        _: &dog_core::TenantContext,
        id: &str,
        params: dog_transport::http::RestParams,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({"id":id,"params":params}))
    }
    async fn custom(
        &self,
        _: &dog_core::TenantContext,
        method: &str,
        data: Option<serde_json::Value>,
        params: dog_transport::http::RestParams,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({"method":method,"data":data,"params":params}))
    }
    fn capabilities(&self) -> dog_core::ServiceCapabilities {
        dog_core::ServiceCapabilities::from_methods(vec![
            dog_core::ServiceMethodKind::Get,
            dog_core::ServiceMethodKind::Custom("echo"),
        ])
    }
}

#[tokio::test]
async fn alias_mount_preserves_decoded_id_and_original_uri() {
    let app = dog_axum::axum(dog_core::DogApp::default()).use_service_as(
        "/api/people",
        "users",
        std::sync::Arc::new(Echo),
    );
    let res = app
        .router
        .oneshot(
            Request::builder()
                .uri("/api/people/a%20b?q=one%20two")
                .header("x-service-method", "echo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 10000).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["id"], "a b");
    assert_eq!(value["params"]["path"], "/api/people/a%20b");
    assert_eq!(value["params"]["query"]["q"], "one two");
}

#[tokio::test]
async fn custom_helper_and_tower_transport_have_same_parameters() {
    use dog_transport::{http::DogHttpService, HttpOptions};
    let app = dog_core::DogApp::default();
    app.register_service("users", std::sync::Arc::new(Echo));
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-service-method", "echo".parse().unwrap());
    let uri = "/users?q=hello+world".parse().unwrap();
    let via_helper = dog_axum::rest::call_custom(
        &app,
        "users",
        "echo",
        &headers,
        [("q".into(), "hello world".into())].into(),
        "POST",
        &uri,
        Some(serde_json::json!({"x":1})),
    )
    .await
    .unwrap();
    let res = DogHttpService::new(app, HttpOptions::default())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("x-service-method", "echo")
                .body(Body::from(r#"{"x":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(Body::new(res.into_body()), 10000)
        .await
        .unwrap();
    let via_transport: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(via_helper, via_transport);
}

#[tokio::test]
async fn upload_processors_survive_clone_and_temporary_file_is_cleaned() {
    use axum::Json;
    let config = MultipartConfig::new().global_processor(|ctx| {
        ctx.data = b"processed".to_vec();
        Ok(())
    });
    let router = Router::new()
        .route(
            "/upload",
            post(|Json(value): Json<serde_json::Value>| async move {
                let path = value["file"]["temp_path"].as_str().unwrap();
                assert_eq!(tokio::fs::read(path).await.unwrap(), b"processed");
                path.to_owned()
            }),
        )
        .layer(MultipartToJson::with_config(config));
    let res = router.oneshot(Request::builder().method("POST").uri("/upload")
        .header("content-type", "multipart/form-data; boundary=\"test\"")
        .body(Body::from("--test\r\nContent-Disposition: form-data; name=\"file\"; filename=\"input.txt\"\r\nContent-Type: text/plain\r\n\r\noriginal\r\n--test--\r\n")).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 10000).await.unwrap();
    let path = std::str::from_utf8(&bytes).unwrap();
    assert!(!std::path::Path::new(path).exists());
}

fn file_request(content_type: bool) -> Request<Body> {
    Request::builder().method("POST").uri("/upload")
        .header("content-type", "multipart/form-data; boundary=test")
        .body(Body::from(format!("--test\r\nContent-Disposition: form-data; name=\"file\"; filename=\"input.txt\"\r\n{}\r\noriginal\r\n--test--\r\n", if content_type { "Content-Type: text/plain\r\n" } else { "" }))).unwrap()
}

#[tokio::test]
async fn cancelled_upload_handler_cleans_temporary_file() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let router = Router::new()
        .route(
            "/upload",
            post(move |axum::Json(value): axum::Json<serde_json::Value>| {
                let tx = tx.clone();
                async move {
                    tx.send(value["file"]["temp_path"].as_str().unwrap().to_owned())
                        .await
                        .unwrap();
                    std::future::pending::<StatusCode>().await
                }
            }),
        )
        .layer(MultipartToJson::new());
    let task = tokio::spawn(router.oneshot(file_request(true)));
    let path = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(std::path::Path::new(&path).exists());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(!std::path::Path::new(&path).exists());
}

#[tokio::test]
async fn file_limit_and_missing_allowed_mime_reject_before_handler() {
    for (config, mime, expected) in [
        (
            MultipartConfig::new().max_file_size(1),
            true,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            MultipartConfig::new().allow_content_type("text/plain"),
            false,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
    ] {
        let router = Router::new()
            .route("/upload", post(|| async { StatusCode::IM_A_TEAPOT }))
            .layer(MultipartToJson::with_config(config));
        assert_eq!(
            router.oneshot(file_request(mime)).await.unwrap().status(),
            expected
        );
    }
}

#[tokio::test]
async fn configured_file_encoding_is_honored() {
    use dog_axum::middlewares::multipart::FileEncoding;
    for encoding in [
        FileEncoding::Base64,
        FileEncoding::Metadata,
        FileEncoding::Skip,
    ] {
        let router = Router::new()
            .route(
                "/upload",
                post(|body: axum::Json<serde_json::Value>| async move { body }),
            )
            .layer(MultipartToJson::with_config(
                MultipartConfig::new().file_encoding(encoding.clone()),
            ));
        let res = router.oneshot(file_request(true)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 10000).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        match encoding {
            FileEncoding::Base64 => assert_eq!(value["file"]["data"], "b3JpZ2luYWw="),
            FileEncoding::Metadata => {
                assert_eq!(value["file"]["size"], 8);
                assert!(value["file"].get("data").is_none());
            }
            FileEncoding::Skip => assert!(value["file"].is_null()),
            _ => unreachable!(),
        }
        assert!(value["file"].get("temp_path").is_none());
    }
}

#[tokio::test]
async fn compatibility_head_uses_host_resolved_get_and_has_no_body() {
    let app = dog_axum::axum(dog_core::DogApp::default()).use_service_as(
        "/people",
        "users",
        std::sync::Arc::new(Echo),
    );
    let res = app
        .router
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/people/123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(axum::body::to_bytes(res.into_body(), 10000)
        .await
        .unwrap()
        .is_empty());
}
