use axum::{body::to_bytes, response::IntoResponse};
use dog_axum::DogAxumError;
#[tokio::test]
async fn internal_error_details_must_not_reach_client() {
    let response =
        DogAxumError(anyhow::anyhow!("internal-db-password=audit-only-sentinel")).into_response();
    let body = to_bytes(response.into_body(), 10240).await.unwrap();
    assert!(
        !String::from_utf8_lossy(&body).contains("audit-only-sentinel"),
        "raw internal error message exposed in HTTP response"
    );
}
