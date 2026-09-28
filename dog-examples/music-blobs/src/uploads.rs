//! Loopback single-tenant streaming HTTP endpoints. Authenticate before exposing remotely.
use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use dog_blob::{BlobAdapter, BlobCtx, BlobKeyStrategy, BlobPut, BlobStore};
use futures::TryStreamExt;
use std::sync::Arc;

#[derive(Clone)]
struct UploadState {
    adapter: Arc<BlobAdapter>,
    store: Arc<dyn BlobStore>,
    receipts: std::path::PathBuf,
}
pub fn router(
    app: &dog_core::DogApp<serde_json::Value, crate::MusicParams>,
) -> anyhow::Result<Router> {
    let state = app
        .get::<Arc<crate::rustfs::RustFsState>>("rustfs")
        .ok_or_else(|| anyhow::anyhow!("missing blob state"))?;
    Ok(Router::new()
        .route("/uploads", post(upload))
        .route("/blobs/{id}", get(download))
        .with_state(UploadState {
            adapter: Arc::new(BlobAdapter::new(state.blob_state.clone())),
            store: Arc::new(state.rustfs_store.clone()),
            receipts: state.receipts_directory.clone(),
        }))
}
fn error(e: impl Into<anyhow::Error>) -> Response {
    let e = e.into();
    let status = match e.downcast_ref::<dog_blob::BlobError>() {
        Some(dog_blob::BlobError::ResourceLimit { .. }) => StatusCode::TOO_MANY_REQUESTS,
        Some(dog_blob::BlobError::Timeout { .. }) => StatusCode::REQUEST_TIMEOUT,
        Some(dog_blob::BlobError::NotFound { .. }) => StatusCode::NOT_FOUND,
        Some(dog_blob::BlobError::Invalid { .. }) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    tracing::warn!("blob request failed: {e}");
    (
        status,
        "Blob operation failed; check the request and server logs",
    )
        .into_response()
}
async fn upload(State(state): State<UploadState>, headers: HeaderMap, body: Body) -> Response {
    let filename = headers
        .get("x-filename")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("upload");
    if filename.len() > 255 {
        return (StatusCode::BAD_REQUEST, "filename too long").into_response();
    }
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");
    if ![
        "audio/mpeg",
        "audio/wav",
        "audio/x-wav",
        "audio/flac",
        "audio/aac",
        "audio/ogg",
        "audio/mp4",
        "application/octet-stream",
    ]
    .contains(&content_type)
    {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "upload an audio file").into_response();
    }
    let put = BlobPut::new().with_filename(filename).with_content_type(
        headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream"),
    );
    let result = state
        .adapter
        .put(
            BlobCtx::new("default".into()),
            put,
            Box::pin(body.into_data_stream().map_err(std::io::Error::other)),
        )
        .await;
    match result {
        Ok(receipt) => {
            if let Err(e) = crate::receipts::persist(&state.receipts, &receipt).await {
                return error(e);
            }
            if let Some(id) = &receipt.recovery_id {
                if let Err(e) = state.adapter.acknowledge_write(id).await {
                    return error(e);
                }
            }
            Json(receipt).into_response()
        }
        Err(e) => error(e),
    }
}
async fn download(
    State(state): State<UploadState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let range = match headers.get("range") {
        None => None,
        Some(value) => {
            let parsed = value
                .to_str()
                .ok()
                .and_then(|s| s.strip_prefix("bytes="))
                .and_then(|s| s.split_once('-'))
                .and_then(|(start, end)| {
                    Some(dog_blob::ByteRange {
                        start: start.parse().ok()?,
                        end: if end.is_empty() {
                            None
                        } else {
                            Some(end.parse().ok()?)
                        },
                    })
                });
            if parsed.is_none() {
                return (
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    "use a single bytes=start-end range",
                )
                    .into_response();
            }
            parsed
        }
    };
    if uuid::Uuid::parse_str(&id).is_err() {
        return (StatusCode::BAD_REQUEST, "invalid blob ID").into_response();
    }
    let key = dog_blob::DefaultKeyStrategy.object_key("default", &id, &Default::default());
    if let Some(requested) = &range {
        match state.store.head(&key).await {
            Ok(head) if !requested.is_valid(head.size_bytes) => {
                return (
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    "range is outside the object",
                )
                    .into_response()
            }
            Err(e) => return error(e),
            _ => {}
        }
    }
    match state.store.get(&key, range).await {
        Ok(opened) => {
            let mut builder = Response::builder()
                .header("accept-ranges", "bytes")
                .header("x-content-type-options", "nosniff")
                .header(
                    "content-type",
                    opened
                        .content_type
                        .unwrap_or_else(|| "application/octet-stream".into()),
                );
            if let Some(range) = opened.resolved_range {
                builder = builder
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(
                        "content-range",
                        format!("bytes {}-{}/{}", range.start, range.end, range.total_size),
                    )
                    .header("content-length", opened.size_bytes);
            } else {
                builder = builder.header("content-length", opened.size_bytes);
            }
            builder
                .body(Body::from_stream(opened.stream))
                .unwrap_or_else(error)
        }
        Err(e) => error(e),
    }
}

/// Inspect journal entries and reconcile incomplete provider uploads under exclusive leases.
/// Retain records for operator review; never erase a committed object or guess its business outcome.
pub async fn recover(
    app: &dog_core::DogApp<serde_json::Value, crate::MusicParams>,
) -> anyhow::Result<()> {
    let state = app
        .get::<Arc<crate::rustfs::RustFsState>>("rustfs")
        .ok_or_else(|| anyhow::anyhow!("missing blob state"))?;
    let adapter = BlobAdapter::new(state.blob_state.clone());
    for pending in adapter.pending_writes().await? {
        match adapter.reconcile_write(&pending.id).await? {
            Some(report) => println!("{} {:?} {}", pending.id, report.outcome, pending.key),
            None => println!("{} busy", pending.id),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use dog_blob::{BlobConfig, BlobState, FileUploadJournal, S3CompatibleStore, S3Config};
    use tower::ServiceExt;

    #[tokio::test]
    #[ignore = "requires disposable loopback RustFS with fixed test credentials"]
    async fn live_upload_download_range_and_receipt() -> anyhow::Result<()> {
        let endpoint = std::env::var("DOGRS_BLOB_TEST_ENDPOINT")?;
        let port = endpoint
            .strip_prefix("http://127.0.0.1:")
            .ok_or_else(|| anyhow::anyhow!("loopback only"))?;
        port.parse::<u16>()?;
        let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new("us-east-1"))
            .credentials_provider(aws_credential_types::Credentials::new(
                "dogrs-local-test",
                "dogrs-local-test-only",
                None,
                None,
                "test",
            ))
            .endpoint_url(&endpoint)
            .load()
            .await;
        let client = aws_sdk_s3::Client::from_conf(
            aws_sdk_s3::config::Builder::from(&sdk)
                .force_path_style(true)
                .build(),
        );
        let bucket = format!("dogrs-example-{}", uuid::Uuid::new_v4());
        client.create_bucket().bucket(&bucket).send().await?;
        let store = S3CompatibleStore::with_config(
            bucket.clone(),
            S3Config {
                region: "us-east-1".into(),
                endpoint_url: endpoint,
                access_key_id: "dogrs-local-test".into(),
                secret_access_key: "dogrs-local-test-only".into(),
            },
        )
        .await;
        let directory = tempfile::tempdir()?;
        let adapter = Arc::new(BlobAdapter::new(Arc::new(
            BlobState::new(
                store.clone(),
                BlobConfig {
                    multipart_threshold_bytes: 5 * 1024 * 1024,
                    ..Default::default()
                },
            )
            .with_journal(Arc::new(FileUploadJournal::new(
                directory.path().join("journal"),
            )?)),
        )));
        let router = Router::new()
            .route("/uploads", post(upload))
            .route("/blobs/{id}", get(download))
            .with_state(UploadState {
                adapter: adapter.clone(),
                store: Arc::new(store),
                receipts: directory.path().to_owned(),
            });
        // Cross the native multipart threshold using an unknown-length request stream.
        let data = bytes::Bytes::from(vec![42; 6 * 1024 * 1024]);
        let response = router
            .clone()
            .oneshot(
                Request::post("/uploads")
                    .header("x-filename", "test.bin")
                    .body(Body::from_stream(futures::stream::iter(vec![Ok::<
                        _,
                        std::io::Error,
                    >(
                        data.clone(),
                    )])))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let receipt: dog_blob::BlobReceipt =
            serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 16 * 1024).await?)?;
        assert!(receipt.recovery_id.is_some());
        assert_eq!(receipt.size_bytes, data.len() as u64);
        assert!(directory
            .path()
            .join(format!("{}.json", receipt.id))
            .exists());
        assert!(
            adapter.pending_writes().await?.is_empty(),
            "receipt must be durable before journal acknowledgment"
        );
        let response = router
            .clone()
            .oneshot(
                Request::get(format!("/blobs/{}", receipt.id))
                    .header("range", "bytes=10-19")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.headers()["content-range"],
            format!("bytes 10-19/{}", data.len())
        );
        assert_eq!(
            axum::body::to_bytes(response.into_body(), 100).await?,
            &data[10..20]
        );
        let response = router
            .oneshot(Request::get(format!("/blobs/{}", receipt.id)).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            axum::body::to_bytes(response.into_body(), data.len() + 1).await?,
            data
        );
        adapter
            .delete(BlobCtx::new("default".into()), receipt.id)
            .await?;
        client.delete_bucket().bucket(bucket).send().await?;
        Ok(())
    }
}
