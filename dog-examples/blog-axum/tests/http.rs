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
    let ax = build().await.unwrap();

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
    let ax = build().await.unwrap();

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
    let ax = build().await.unwrap();

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
    let ax = build().await.unwrap();

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

#[tokio::test]
async fn posts_are_isolated_by_tenant() {
    let ax = build().await.unwrap();

    // Create a published post in tenant A
    let _ = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("x-tenant-id", "tenant-a")
                .header("content-type", "application/json")
                .body(Body::from(
                    "{\"title\":\"A\",\"body\":\"x\",\"published\":true}",
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Tenant A can see it
    let res = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/posts")
                .header("x-tenant-id", "tenant-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let body = json_body(res).await;
    assert_eq!(body.as_array().unwrap().len(), 1);

    // Tenant B cannot see tenant A's post
    let res = ax
        .router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/posts")
                .header("x-tenant-id", "tenant-b")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let body = json_body(res).await;
    assert_eq!(body.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn posts_create_with_invalid_author_id_is_422() {
    let ax = build().await.unwrap();

    let res = ax
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .body(Body::from(
                    "{\"title\":\"Hello\",\"body\":\"x\",\"author_id\":\"author:does-not-exist\"}",
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status().as_u16(), 422);
    let body = json_body(res).await;
    assert_eq!(body["name"], "Unprocessable");
    assert_eq!(body["errors"]["author_id"][0], "author not found");
}

#[tokio::test]
async fn posts_find_expand_author_embeds_author() {
    let ax = build().await.unwrap();

    // Create author
    let author_res = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/authors")
                .header("content-type", "application/json")
                .body(Body::from(
                    "{\"name\":\"Alice\",\"email\":\"alice@example.com\",\"profile\":{\"display_name\":\"Alice A\"},\"tags\":[{\"email\":\"tag@example.com\"}]}",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(author_res.status().as_u16(), 200);
    let author = json_body(author_res).await;
    let author_id = author["id"].as_str().unwrap().to_string();

    // Create post referencing author
    let post_res = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    "{{\"title\":\"Hello\",\"body\":\"x\",\"author_id\":\"{}\",\"published\":true}}",
                    author_id
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post_res.status().as_u16(), 200);

    // Find with expand
    let res = ax
        .router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/posts?expand=author")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let body = json_body(res).await;
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["author"]["id"], Value::String(author_id));
}

#[tokio::test]
async fn authors_on_delete_restrict_blocks_when_posts_reference() {
    let ax = build().await.unwrap();

    // Create author
    let author_res = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/authors")
                .header("content-type", "application/json")
                .body(Body::from(
                    "{\"name\":\"Alice\",\"email\":\"alice@example.com\",\"profile\":{\"display_name\":\"Alice A\"},\"tags\":[{\"email\":\"tag@example.com\"}]}",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let author = json_body(author_res).await;
    let author_id = author["id"].as_str().unwrap().to_string();

    // Create published post referencing author
    let _ = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    "{{\"title\":\"Hello\",\"body\":\"x\",\"author_id\":\"{}\",\"published\":true}}",
                    author_id
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    // Attempt delete with restrict
    let res = ax
        .router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/authors/{}?onDelete=restrict", author_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status().as_u16(), 409);
}

#[tokio::test]
async fn authors_on_delete_cascade_removes_posts() {
    let ax = build().await.unwrap();

    let author_res = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/authors")
                .header("content-type", "application/json")
                .body(Body::from(
                    "{\"name\":\"Alice\",\"email\":\"alice@example.com\",\"profile\":{\"display_name\":\"Alice A\"},\"tags\":[{\"email\":\"tag@example.com\"}]}",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let author = json_body(author_res).await;
    let author_id = author["id"].as_str().unwrap().to_string();

    let _ = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    "{{\"title\":\"Hello\",\"body\":\"x\",\"author_id\":\"{}\",\"published\":true}}",
                    author_id
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    let res = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/authors/{}?onDelete=cascade", author_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);

    let res = ax
        .router
        .oneshot(Request::builder().method("GET").uri("/posts").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let body = json_body(res).await;
    assert_eq!(body.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn authors_on_delete_nullify_clears_author_id_on_posts() {
    let ax = build().await.unwrap();

    let author_res = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/authors")
                .header("content-type", "application/json")
                .body(Body::from(
                    "{\"name\":\"Alice\",\"email\":\"alice@example.com\",\"profile\":{\"display_name\":\"Alice A\"},\"tags\":[{\"email\":\"tag@example.com\"}]}",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let author = json_body(author_res).await;
    let author_id = author["id"].as_str().unwrap().to_string();

    let _ = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    "{{\"title\":\"Hello\",\"body\":\"x\",\"author_id\":\"{}\",\"published\":true}}",
                    author_id
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    let res = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/authors/{}?onDelete=nullify", author_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);

    let res = ax
        .router
        .oneshot(Request::builder().method("GET").uri("/posts").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let body = json_body(res).await;
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert!(arr[0].get("author_id").is_none());
}

#[tokio::test]
async fn authors_nested_validation_errors_have_world_class_paths() {
    let ax = build().await.unwrap();

    let res = ax
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/authors")
                .header("content-type", "application/json")
                .body(Body::from(
                    "{\"name\":\"Alice\",\"email\":\"alice@example.com\",\"profile\":{\"display_name\":\"x\"},\"tags\":[{\"email\":\"not-an-email\"}]}",
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status().as_u16(), 422);
    let body = json_body(res).await;

    assert_eq!(body["errors"]["profile.display_name"][0], "display_name must be at least 2 chars");
    assert_eq!(body["errors"]["tags[0].email"][0], "tag email must be a valid email");
}

#[tokio::test]
async fn authors_create_missing_name_is_422() {
    let ax = build().await.unwrap();

    let res = ax
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/authors")
                .header("content-type", "application/json")
                .body(Body::from("{\"email\":\"a@example.com\"}"))
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
async fn authors_are_isolated_by_tenant() {
    let ax = build().await.unwrap();

    let _ = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/authors")
                .header("x-tenant-id", "tenant-a")
                .header("content-type", "application/json")
                .body(Body::from(
                    "{\"name\":\"Alice\",\"email\":\"alice@example.com\",\"profile\":{\"display_name\":\"Alice A\"},\"tags\":[{\"email\":\"tag@example.com\"}]}",
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let res = ax
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/authors")
                .header("x-tenant-id", "tenant-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let body = json_body(res).await;
    assert_eq!(body.as_array().unwrap().len(), 1);

    let res = ax
        .router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/authors")
                .header("x-tenant-id", "tenant-b")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let body = json_body(res).await;
    assert_eq!(body.as_array().unwrap().len(), 0);
}
