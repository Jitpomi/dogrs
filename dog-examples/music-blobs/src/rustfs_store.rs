use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region};
use aws_credential_types::Credentials;
use aws_sdk_s3::{primitives::ByteStream as AwsByteStream, Client};
use futures::StreamExt;
use std::env;

use dog_blob::{
    BlobError, BlobResult, BlobStore, ByteRange, ByteStream, GetResult, ObjectHead, PutResult,
    StoreCapabilities,
};

/// RustFS configuration from environment variables
pub struct RustFSConfig {
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub endpoint_url: String,
}

impl RustFSConfig {
    pub fn from_env() -> BlobResult<Self> {
        let region = env::var("RUSTFS_REGION")
            .map_err(|_| BlobError::invalid("RUSTFS_REGION environment variable required"))?;
        let access_key_id = env::var("RUSTFS_ACCESS_KEY_ID").map_err(|_| {
            BlobError::invalid("RUSTFS_ACCESS_KEY_ID environment variable required")
        })?;
        let secret_access_key = env::var("RUSTFS_SECRET_ACCESS_KEY").map_err(|_| {
            BlobError::invalid("RUSTFS_SECRET_ACCESS_KEY environment variable required")
        })?;
        let endpoint_url = env::var("RUSTFS_ENDPOINT_URL")
            .map_err(|_| BlobError::invalid("RUSTFS_ENDPOINT_URL environment variable required"))?;

        Ok(RustFSConfig {
            region,
            access_key_id,
            secret_access_key,
            endpoint_url,
        })
    }
}

/// Production RustFS store implementation using AWS SDK (S3-compatible)
#[derive(Clone)]
pub struct RustFSStore {
    client: Client,
    bucket: String,
}

impl RustFSStore {
    pub async fn new(bucket: String) -> BlobResult<Self> {
        let config = RustFSConfig::from_env()?;

        let credentials = Credentials::new(
            config.access_key_id,
            config.secret_access_key,
            None,
            None,
            "rustfs",
        );

        let region = Region::new(config.region);

        let aws_config = aws_config::defaults(BehaviorVersion::latest())
            .region(region)
            .credentials_provider(credentials)
            .endpoint_url(config.endpoint_url)
            .load()
            .await;

        let client = Client::from_conf(
            aws_sdk_s3::config::Builder::from(&aws_config)
                .force_path_style(true) // Required for RustFS compatibility
                .build(),
        );

        Ok(Self { client, bucket })
    }
}

#[async_trait]
impl BlobStore for RustFSStore {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    async fn put(
        &self,
        key: &str,
        content_type: Option<&str>,
        mut stream: ByteStream,
    ) -> BlobResult<PutResult> {
        println!("🗄️  RustFS storage operation:");
        println!("   Key: {}", key);
        println!("   Content-Type: {:?}", content_type);
        // Collect stream into bytes for AWS SDK
        println!("   Collecting stream chunks...");
        let mut data = Vec::new();
        let mut chunk_count = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| BlobError::backend(e))?;
            chunk_count += 1;
            println!("   Chunk {}: {} bytes", chunk_count, chunk.len());
            data.extend_from_slice(&chunk);
        }
        println!(
            "   Total collected: {} bytes from {} chunks",
            data.len(),
            chunk_count
        );

        let aws_stream = AwsByteStream::from(data.clone());

        let mut request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(aws_stream);

        if let Some(ct) = content_type {
            request = request.content_type(ct);
        }

        let result = request.send().await.map_err(|e| BlobError::backend(e))?;

        Ok(PutResult {
            etag: result.e_tag,
            size_bytes: data.len() as u64,
            checksum: None,
        })
    }

    async fn get(&self, key: &str, range: Option<ByteRange>) -> BlobResult<GetResult> {
        let mut request = self.client.get_object().bucket(&self.bucket).key(key);

        if let Some(ref range) = range {
            let range_str = if let Some(end) = range.end {
                format!("bytes={}-{}", range.start, end)
            } else {
                format!("bytes={}-", range.start)
            };
            request = request.range(range_str);
        }

        let result = request.send().await.map_err(|e| BlobError::backend(e))?;

        let body = result
            .body
            .collect()
            .await
            .map_err(|e| BlobError::backend(e))?;

        let stream = futures::stream::once(async move { Ok(body.into_bytes()) });

        Ok(GetResult {
            stream: Box::pin(stream),
            size_bytes: result.content_length.unwrap_or(0) as u64,
            content_type: result.content_type,
            etag: result.e_tag,
            resolved_range: range.as_ref().map(|r| dog_blob::store::ResolvedRange {
                start: r.start,
                end: r
                    .end
                    .unwrap_or(result.content_length.unwrap_or(0) as u64 - 1),
                total_size: result.content_length.unwrap_or(0) as u64,
            }),
        })
    }

    async fn head(&self, key: &str) -> BlobResult<ObjectHead> {
        let result = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| BlobError::backend(e))?;

        Ok(ObjectHead {
            size_bytes: result.content_length.unwrap_or(0) as u64,
            content_type: result.content_type,
            etag: result.e_tag,
            last_modified: result.last_modified.map(|dt| dt.secs()),
        })
    }

    async fn delete(&self, key: &str) -> BlobResult<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| BlobError::backend(e))?;
        Ok(())
    }

    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities::basic().with_range().with_signed_urls()
    }
}
