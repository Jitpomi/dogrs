//! Disposable loopback RustFS only; never uses developer/cloud credentials.
#![cfg(feature = "s3")]
use dog_blob::*;
use futures_util::StreamExt;
use std::sync::Arc;
fn body(value: &[u8]) -> ByteStream {
    let value = bytes::Bytes::copy_from_slice(value);
    Box::pin(futures::stream::once(async { Ok(value) }))
}
#[tokio::test]
#[ignore = "requires disposable local RustFS; see README"]
async fn real_s3_streams_ranges_metadata_limits_and_signed_reads() {
    let endpoint = std::env::var("DOGRS_BLOB_TEST_ENDPOINT").expect("local RustFS endpoint");
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
    let journal_dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(FileUploadJournal::new(journal_dir.path()).unwrap());
    let resources = UploadResources::new(UploadLimits {
        max_staging_bytes: 32 * 1024 * 1024,
        ..Default::default()
    })
    .unwrap();
    let adapter = BlobAdapter::new(Arc::new(
        BlobState::new(store.clone(), BlobConfig::default().with_checksum("sha256"))
            .with_resources(resources.clone())
            .with_journal(journal.clone()),
    ));
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
    assert_eq!(resources.usage().staged_files, 1);
    assert_eq!(resources.usage().peak_staging_bytes, receipt.size_bytes);
    assert_eq!(resources.usage().staging_bytes, 0);
    let write_id = receipt
        .recovery_id
        .as_deref()
        .expect("native journal identity");
    assert!(
        receipt.etag.as_ref().unwrap().contains('-'),
        "provider multipart etag"
    );
    let recovered = BlobAdapter::new(Arc::new(
        BlobState::new(store.clone(), BlobConfig::default()).with_journal(Arc::new(
            FileUploadJournal::new(journal_dir.path()).unwrap(),
        )),
    ));
    assert_eq!(
        recovered
            .reconcile_write(write_id)
            .await
            .unwrap()
            .unwrap()
            .outcome,
        WriteOutcome::Committed
    );
    assert!(recovered.acknowledge_write(write_id).await.unwrap());
    assert!(recovered.pending_writes().await.unwrap().is_empty());
    // Persist an initiated upload, then drop its writer as if the process stopped.
    let mut interrupted = PendingWrite {
        id: uuid::Uuid::new_v4().to_string(),
        scope: store.recovery_scope(),
        key: "interrupted".into(),
        size_bytes: 4,
        checksum: "sha256:fixture".into(),
        content_type: None,
        filename: None,
        native_id: None,
    };
    let mut lease = journal.create(interrupted.clone()).await.unwrap();
    interrupted.native_id = Some(store.initiate(&interrupted).await.unwrap());
    lease.save(interrupted.clone()).await.unwrap();
    assert!(recovered
        .reconcile_write(&interrupted.id)
        .await
        .unwrap()
        .is_none());
    drop(lease);
    assert_eq!(
        recovered
            .reconcile_write(&interrupted.id)
            .await
            .unwrap()
            .unwrap()
            .outcome,
        WriteOutcome::Aborted
    );
    assert!(recovered.acknowledge_write(&interrupted.id).await.unwrap());
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
    // The resumable coordinator also forwards validated parts/assembly without
    // causing S3 to stage them again. Share one budget across both components.
    let staged_resources = UploadResources::new(UploadLimits {
        max_staging_bytes: 6,
        ..Default::default()
    })
    .unwrap();
    let mut resumable_config = BlobConfig::default().with_max_blob_bytes(6);
    resumable_config.upload_rules.part_size = 4;
    let coordinator = DefaultUploadCoordinator::new(
        store.clone().with_resources(staged_resources.clone()),
        MemoryUploadSessionStore::new(),
        DefaultKeyStrategy,
        resumable_config,
    )
    .with_resources(staged_resources.clone());
    let id = BlobId::new();
    let key = DefaultKeyStrategy.object_key(&ctx.tenant_id, id.as_str(), &Default::default());
    let session = coordinator
        .begin(
            ctx.clone(),
            UploadIntent::new(id, key).with_parts(4, Some(2)),
        )
        .await
        .unwrap();
    coordinator
        .accept_part(ctx.clone(), &session.upload_id, 1, body(b"1234"))
        .await
        .unwrap();
    coordinator
        .accept_part(ctx.clone(), &session.upload_id, 2, body(b"56"))
        .await
        .unwrap();
    let assembled = coordinator
        .complete(ctx.clone(), &session.upload_id)
        .await
        .unwrap();
    assert_eq!(staged_resources.usage().staged_files, 3);
    assert_eq!(staged_resources.usage().peak_staging_bytes, 6);
    assert_eq!(staged_resources.usage().staging_bytes, 0);
    coordinator
        .forget(ctx.clone(), &session.upload_id)
        .await
        .unwrap();
    store.delete(&assembled.key).await.unwrap();
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
