use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region};
use aws_credential_types::Credentials;
use aws_sdk_s3::{primitives::ByteStream as AwsByteStream, Client};
use futures::StreamExt;
use std::env;

use dog_blob::{
    BlobError, BlobInfo, BlobResult, BlobStore, ByteRange, ByteStream, GetResult, ObjectHead, PutResult,
    StoreCapabilities,
};

/// RustFS configuration from environment variables
#[derive(Debug)]
struct RustFSConfig {
    region: String,
    access_key_id: String,
    secret_access_key: String,
    endpoint_url: String,
}

impl RustFSConfig {
    fn from_env() -> BlobResult<Self> {
        fn get_env(key: &str) -> BlobResult<String> {
            env::var(key).map_err(|_| BlobError::invalid(format!("{} environment variable required", key)))
        }

        Ok(Self {
            region: get_env("RUSTFS_REGION")?,
            access_key_id: get_env("RUSTFS_ACCESS_KEY_ID")?,
            secret_access_key: get_env("RUSTFS_SECRET_ACCESS_KEY")?,
            endpoint_url: get_env("RUSTFS_ENDPOINT_URL")?,
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
        let client = Self::create_client(config).await;
        Ok(Self { client, bucket })
    }

    async fn create_client(config: RustFSConfig) -> Client {
        let credentials = Credentials::new(
            config.access_key_id,
            config.secret_access_key,
            None,
            None,
            "rustfs",
        );

        let aws_config = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(config.region))
            .credentials_provider(credentials)
            .endpoint_url(config.endpoint_url)
            .load()
            .await;

        Client::from_conf(
            aws_sdk_s3::config::Builder::from(&aws_config)
                .force_path_style(true) // Required for RustFS compatibility
                .build(),
        )
    }

    async fn collect_stream(&self, stream: &mut ByteStream) -> BlobResult<Vec<u8>> {
        let mut data = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(Self::map_aws_error)?;
            data.extend_from_slice(&chunk);
        }
        Ok(data)
    }

    fn format_range(&self, range: &ByteRange) -> String {
        match range.end {
            Some(end) => format!("bytes={}-{}", range.start, end),
            None => format!("bytes={}-", range.start),
        }
    }

    fn resolve_range(&self, range: &ByteRange, content_length: u64) -> dog_blob::store::ResolvedRange {
        dog_blob::store::ResolvedRange {
            start: range.start,
            end: range.end.unwrap_or(content_length.saturating_sub(1)),
            total_size: content_length,
        }
    }

    fn map_aws_error(err: impl std::error::Error + Send + Sync + 'static) -> BlobError {
        BlobError::backend(err)
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
        let data = self.collect_stream(&mut stream).await?;
        let aws_stream = AwsByteStream::from(data.clone());

        let mut request = self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(aws_stream);

        if let Some(ct) = content_type {
            request = request.content_type(ct);
        }

        let result = request.send().await.map_err(Self::map_aws_error)?;

        Ok(PutResult {
            etag: result.e_tag,
            size_bytes: data.len() as u64,
            checksum: None,
        })
    }

    async fn put_with_metadata(
        &self,
        key: &str,
        content_type: Option<&str>,
        filename: Option<&str>,
        mut stream: ByteStream,
    ) -> BlobResult<PutResult> {
        let data = self.collect_stream(&mut stream).await?;
        let aws_stream = AwsByteStream::from(data.clone());

        let mut request = self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(aws_stream);

        if let Some(ct) = content_type {
            request = request.content_type(ct);
        }

        // Add filename as metadata if provided
        if let Some(filename) = filename {
            request = request.metadata("filename", filename);
        }

        let result = request.send().await.map_err(Self::map_aws_error)?;

        Ok(PutResult {
            size_bytes: data.len() as u64,
            etag: result.e_tag,
            checksum: None,
        })
    }

    async fn get(&self, key: &str, range: Option<ByteRange>) -> BlobResult<GetResult> {
        let mut request = self.client.get_object().bucket(&self.bucket).key(key);

        if let Some(ref range) = range {
            request = request.range(self.format_range(range));
        }

        let result = request.send().await.map_err(Self::map_aws_error)?;
        let content_length = result.content_length.unwrap_or(0) as u64;

        let body = result.body.collect().await.map_err(Self::map_aws_error)?;
        let stream = futures::stream::once(async move { Ok(body.into_bytes()) });

        Ok(GetResult {
            stream: Box::pin(stream),
            size_bytes: content_length,
            content_type: result.content_type,
            etag: result.e_tag,
            resolved_range: range.as_ref().map(|r| self.resolve_range(r, content_length)),
        })
    }

    async fn head(&self, key: &str) -> BlobResult<ObjectHead> {
        let result = self.client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(Self::map_aws_error)?;

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
            .map_err(Self::map_aws_error)?;
        Ok(())
    }

    async fn list(&self, prefix: Option<&str>, limit: Option<usize>) -> BlobResult<Vec<BlobInfo>> {
        let mut request = self.client
            .list_objects_v2()
            .bucket(&self.bucket);

        if let Some(prefix) = prefix {
            request = request.prefix(prefix);
        }

        if let Some(limit) = limit {
            request = request.max_keys(limit as i32);
        }

        let result = request.send().await.map_err(Self::map_aws_error)?;

        let mut blobs = Vec::new();
        if let Some(objects) = result.contents {
            for object in objects {
                if let Some(key) = object.key {
                    // Get additional metadata including filename from head_object
                    let head_result = self.client
                        .head_object()
                        .bucket(&self.bucket)
                        .key(&key)
                        .send()
                        .await
                        .map_err(Self::map_aws_error)?;

                    // Extract filename from metadata if available
                    let filename = head_result.metadata()
                        .and_then(|metadata| metadata.get("filename"))
                        .map(|f| f.to_string());

                    blobs.push(BlobInfo {
                        key: key.clone(),
                        size_bytes: object.size.unwrap_or(0) as u64,
                        content_type: head_result.content_type,
                        filename,
                        etag: object.e_tag,
                        last_modified: object.last_modified.map(|dt| dt.secs()),
                    });
                }
            }
        }

        Ok(blobs)
    }

    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities::basic().with_range().with_signed_urls()
    }
}
