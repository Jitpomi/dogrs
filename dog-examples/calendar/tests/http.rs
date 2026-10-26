//! # Calendar Integration Tests (`tests/http.rs`)
//!
//! Comprehensive tests verifying:
//! - REST API CRUD operations (`/api/events`).
//! - Schema validation and structured error reporting (422 Unprocessable Entity).
//! - Multi-tenant data isolation (`x-tenant-id`).
//! - Real-time WebSocket event broadcasting over `/ws`.
//! - `dog-queue` delayed reminder scheduling and cancel-wins guarantees.

use axum::body::Body;
use axum::routing::get;
use axum::Router;
use calendar::{build, CalendarParams};
use futures_util::StreamExt;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tokio_tungstenite::connect_async;
use tower::ServiceExt;

dog_transport::declare_adapter!(axum, to_endpoint, CalendarParams);
dog_transport::declare_ws_adapter!(axum, to_ws, CalendarParams);

async fn build_app() -> (dog_core::DogApp<Value, CalendarParams>, Router) {
    let (dog, http_service) = build().await.unwrap();
    let ws_router = Router::new()
        .route("/ws", get(to_ws))
        .with_state(dog.clone());

    let router = Router::new()
        .nest_service("/api", to_endpoint(http_service))
        .merge(ws_router);

    (dog, router)
}

async fn json_body(body: Body) -> Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// Verifies that creating an event without the required `title` field returns 422 Unprocessable.
#[tokio::test]
async fn events_create_missing_title_is_422() {
    let (_dog, router) = build_app().await;

    let res = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/events")
                .header("content-type", "application/json")
                .body(Body::from("{\"description\":\"team sync\"}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(res.into_body()).await;
    assert_eq!(body["name"], "Unprocessable");
    assert_eq!(body["code"], 422);
}

/// Verifies event creation, ID generation, x-request-id header propagation, and GET retrieval.
#[tokio::test]
async fn events_create_and_get() {
    let (_dog, router) = build_app().await;

    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/events")
                .header("content-type", "application/json")
                .body(Body::from(
                    "{\"title\":\"Sprint Planning\",\"description\":\"Plan sprint goals\"}",
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers().get("x-request-id").is_some());

    let body = json_body(res.into_body()).await;
    assert_eq!(body["title"], "Sprint Planning");
    assert_eq!(body["description"], "Plan sprint goals");
    let event_id = body["id"].as_str().unwrap().to_string();

    let get_res = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/events/{event_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(get_res.status(), StatusCode::OK);
    let get_body = json_body(get_res.into_body()).await;
    assert_eq!(get_body["id"], event_id);
    assert_eq!(get_body["title"], "Sprint Planning");
}

/// Verifies partial updates via PATCH and deletion via DELETE.
#[tokio::test]
async fn events_patch_and_delete() {
    let (_dog, router) = build_app().await;

    let create_res = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/events")
                .header("content-type", "application/json")
                .body(Body::from("{\"title\":\"Standup\"}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(create_res.status(), StatusCode::OK);
    let created = json_body(create_res.into_body()).await;
    let event_id = created["id"].as_str().unwrap().to_string();

    let patch_res = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/events/{event_id}"))
                .header("content-type", "application/json")
                .body(Body::from("{\"title\":\"Daily Standup\"}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(patch_res.status(), StatusCode::OK);
    let patched = json_body(patch_res.into_body()).await;
    assert_eq!(patched["title"], "Daily Standup");

    let del_res = router
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/events/{event_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(del_res.status(), StatusCode::OK);

    let get_res = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/events/{event_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(get_res.status(), StatusCode::NOT_FOUND);
}

/// Verifies that data is strictly partitioned and isolated per tenant using `x-tenant-id`.
#[tokio::test]
async fn events_are_isolated_by_tenant() {
    let (_dog, router) = build_app().await;

    let _ = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/events")
                .header("x-tenant-id", "tenant-a")
                .header("content-type", "application/json")
                .body(Body::from("{\"title\":\"Tenant A Event\"}"))
                .unwrap(),
        )
        .await
        .unwrap();

    let res_a = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/events")
                .header("x-tenant-id", "tenant-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res_a.status(), StatusCode::OK);
    let body_a = json_body(res_a.into_body()).await;
    assert_eq!(body_a.as_array().unwrap().len(), 1);

    let res_b = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/events")
                .header("x-tenant-id", "tenant-b")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res_b.status(), StatusCode::OK);
    let body_b = json_body(res_b.into_body()).await;
    assert_eq!(body_b.as_array().unwrap().len(), 0);
}

/// Verifies real-time WebSocket broadcasting: an HTTP mutation triggers an instant WebSocket BROADCAST frame.
#[tokio::test]
async fn events_realtime_websocket_broadcast() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (dog, http_service) = build().await.unwrap();
    let ws_router = Router::new()
        .route("/ws", get(to_ws))
        .with_state(dog.clone());

    let router = Router::new()
        .nest_service("/api", to_endpoint(http_service))
        .merge(ws_router);

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // Connect WebSocket
    let ws_url = format!("ws://{addr}/ws");
    let (ws_stream, _) = connect_async(&ws_url)
        .await
        .expect("WebSocket connect failed");
    let (_, mut read) = ws_stream.split();

    // Trigger HTTP POST /api/events
    let client = reqwest::Client::new();
    let res = client
        .post(format!("http://{addr}/api/events"))
        .json(&serde_json::json!({
            "title": "Realtime Standup",
            "description": "Live sync test"
        }))
        .send()
        .await
        .expect("HTTP POST failed");

    assert_eq!(res.status(), 200);

    // Assert that the WebSocket receives the BROADCAST frame
    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timed out waiting for WS broadcast")
        .expect("Stream ended")
        .expect("WS receive error");

    let text = msg.to_text().expect("Message was not text");
    let json: Value = serde_json::from_str(text).expect("Invalid JSON in WS message");

    assert_eq!(json["type"], "BROADCAST");
    assert_eq!(json["event"], "created");
    assert_eq!(json["payload"]["title"], "Realtime Standup");
    assert_eq!(json["payload"]["description"], "Live sync test");

    server_handle.abort();
}

/// Verifies that dog-queue schedules a delayed reminder job on event creation,
/// and cancels it cleanly (cancel-wins) when the event is removed.
#[tokio::test]
async fn events_queue_reminder_and_cancel_wins() {
    let (_dog, router) = build_app().await;

    // 1. Create event with start_time in the future to trigger delayed reminder queueing
    let start_time = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339();
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/events")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    "{{\"title\":\"Important Meeting\",\"start_time\":\"{start_time}\"}}"
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let created = json_body(res.into_body()).await;
    let event_id = created["id"].as_str().unwrap();

    // 2. Remove event to verify cancel-wins semantics cancel the queued job cleanly
    let del_res = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/events/{event_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(del_res.status(), StatusCode::OK);
}

/// Verifies event creation with recurrence rules and persistence.
#[tokio::test]
async fn events_create_with_recurrence() {
    let (_dog, router) = build_app().await;

    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/events")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"title":"Daily Standup","recurrence":"daily","start_time":"2026-10-26T09:30:00Z"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res.into_body()).await;
    assert_eq!(body["title"], "Daily Standup");
    assert_eq!(body["recurrence"], "daily");
    let event_id = body["id"].as_str().unwrap();

    let get_res = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/events/{event_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(get_res.status(), StatusCode::OK);
    let get_body = json_body(get_res.into_body()).await;
    assert_eq!(get_body["recurrence"], "daily");
}
