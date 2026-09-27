//! Disposable loopback MinIO only; never uses developer/cloud credentials.
#![cfg(feature = "s3")]
use dog_blob::*;
use futures_util::StreamExt;
use std::sync::Arc;
fn body(value: &[u8]) -> ByteStream {
    let value = bytes::Bytes::copy_from_slice(value);
    Box::pin(futures::stream::once(async { Ok(value) }))
}
#[tokio::test]
#[ignore = "requires disposable local MinIO; see README"]
async fn real_s3_streams_ranges_metadata_limits_and_signed_reads() {
    let endpoint = std::env::var("DOGRS_BLOB_TEST_ENDPOINT").expect("local MinIO endpoint");
    let port = endpoint
        .strip_prefix("http://127.0.0.1:")
        .expect("loopback only");
    assert!(port.parse::<u16>().is_ok());
    let config = || S3Config {
        region: "us-east-1".into(),
        endpoint_url: endpoint.clone(),
        access_key_id: "dogrs-local-test".into(),
        secret_access_key: "dogrs-local-test-only".into(),
    };
    let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(aws_credential_types::Credentials::new(
            "dogrs-local-test",
            "dogrs-local-test-only",
            None,
            None,
            "local-test",
        ))
        .endpoint_url(&endpoint)
        .load()
        .await;
    let client = aws_sdk_s3::Client::from_conf(
        aws_sdk_s3::config::Builder::from(&sdk)
            .force_path_style(true)
            .build(),
    );
    let bucket = format!("dogrs-blob-{}", uuid::Uuid::new_v4());
    client.create_bucket().bucket(&bucket).send().await.unwrap();
    let store = S3CompatibleStore::with_config(bucket.clone(), config()).await;
    let adapter = BlobAdapter::new(Arc::new(BlobState::new(
        store.clone(),
        BlobConfig::default().with_checksum("sha256"),
    )));
    let ctx = BlobCtx::new("tenant-a".into()).with_actor("owner".into());
    // Unknown-length stream: 32 MiB without collecting its contents in memory.
    let stream = futures::stream::iter(
        (0..512).map(|n| Ok(bytes::Bytes::from(vec![(n % 251) as u8; 65536]))),
    );
    let receipt = adapter
        .put(
            ctx.clone(),
            BlobPut::new()
                .with_filename("sample.bin")
                .with_content_type("application/octet-stream"),
            Box::pin(stream),
        )
        .await
        .unwrap();
    assert_eq!(receipt.size_bytes, 32 * 1024 * 1024);
    let mut result = store.get(&receipt.key, None).await.unwrap();
    let mut count = 0usize;
    while let Some(bytes) = result.stream.next().await {
        for byte in bytes.unwrap() {
            assert_eq!(byte, ((count / 65536) % 251) as u8);
            count += 1;
        }
    }
    assert_eq!(count as u64, receipt.size_bytes);
    let ranged = adapter
        .open(
            ctx.clone(),
            receipt.id.clone(),
            Some(ByteRange::new(65530, Some(65545))),
        )
        .await
        .unwrap();
    assert_eq!(ranged.receipt.size_bytes, receipt.size_bytes);
    assert_eq!(ranged.content_length(), 16);
    match ranged.content {
        OpenedContent::Stream {
            mut stream,
            resolved_range,
        } => {
            assert_eq!(resolved_range.unwrap().total_size, receipt.size_bytes);
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                bytes.extend_from_slice(&chunk.unwrap());
            }
            assert_eq!(bytes, [vec![0; 6], vec![1; 10]].concat());
        }
        _ => panic!("range must stream"),
    }
    let tail = store
        .get(
            &receipt.key,
            Some(ByteRange::from_start(receipt.size_bytes - 2)),
        )
        .await
        .unwrap();
    assert_eq!(tail.size_bytes, 2);
    assert_eq!(tail.resolved_range.unwrap().total_size, receipt.size_bytes);
    let listed = adapter.list(ctx.clone(), None, Some(10)).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].filename.as_deref(), Some("sample.bin"));
    assert!(adapter
        .list(BlobCtx::new("tenant".into()), None, None)
        .await
        .unwrap()
        .is_empty());
    let small = adapter
        .put(ctx.clone(), BlobPut::new(), body(b"signed-data"))
        .await
        .unwrap();
    match adapter
        .open(ctx.clone(), small.id.clone(), None)
        .await
        .unwrap()
        .content
    {
        OpenedContent::SignedUrl { url, .. } => {
            let output = std::process::Command::new("curl")
                .args(["--fail", "--silent", "--max-time", "10", &url])
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(output.stdout, b"signed-data");
        }
        _ => panic!("signed URL capability missing"),
    }
    let limited = store.clone().with_max_put_bytes(3);
    assert!(limited.put("oversized", None, body(b"1234")).await.is_err());
    assert!(store.head("oversized").await.is_err());
    store.put("empty", None, body(b"")).await.unwrap();
    assert_eq!(store.head("empty").await.unwrap().size_bytes, 0);
    adapter.delete(ctx.clone(), small.id).await.unwrap();
    adapter.delete(ctx, receipt.id).await.unwrap();
    store.delete("empty").await.unwrap();
    client.delete_bucket().bucket(&bucket).send().await.unwrap();
}
