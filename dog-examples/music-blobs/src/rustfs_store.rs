use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region};
use aws_credential_types::Credentials;
use aws_sdk_s3::{primitives::ByteStream as AwsByteStream, Client};
use futures::StreamExt;
use std::env;

use dog_blob::{
    BlobError, BlobInfo, BlobMetadata, BlobResult, BlobStore, ByteRange, ByteStream, GetResult, ObjectHead, PutResult,
    StoreCapabilities,
};
use dog_blob::store::ResolvedRange;
use crate::metadata::AudioMetadataExtractor;

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

    fn resolve_range(&self, range: &ByteRange, content_length: u64) -> ResolvedRange {
        ResolvedRange {
            start: range.start,
            end: range.end.unwrap_or(content_length.saturating_sub(1)),
            total_size: content_length,
        }
    }

    fn map_aws_error(err: impl std::error::Error + Send + Sync + 'static) -> BlobError {
        BlobError::backend(err)
    }

    /// Add metadata fields to S3 put request
    fn add_metadata_to_request(
        mut request: aws_sdk_s3::operation::put_object::builders::PutObjectFluentBuilder,
        metadata: &BlobMetadata,
    ) -> aws_sdk_s3::operation::put_object::builders::PutObjectFluentBuilder {
        // Helper macro to reduce repetition
        macro_rules! add_optional_metadata {
            ($field:expr, $key:literal) => {
                if let Some(value) = $field {
                    request = request.metadata($key, value);
                }
            };
            ($field:expr, $key:literal, to_string) => {
                if let Some(value) = $field {
                    request = request.metadata($key, &value.to_string());
                }
            };
        }

        add_optional_metadata!(&metadata.title, "title");
        add_optional_metadata!(&metadata.artist, "artist");
        add_optional_metadata!(&metadata.album, "album");
        add_optional_metadata!(&metadata.genre, "genre");
        add_optional_metadata!(metadata.year, "year", to_string);
        add_optional_metadata!(metadata.duration, "duration", to_string);
        add_optional_metadata!(metadata.bitrate, "bitrate", to_string);
        add_optional_metadata!(metadata.sample_rate, "sample_rate", to_string);
        add_optional_metadata!(metadata.channels, "channels", to_string);
        add_optional_metadata!(&metadata.encoding, "encoding");
        
        // Visual metadata
        add_optional_metadata!(&metadata.album_art_url, "album_art_url");
        add_optional_metadata!(&metadata.thumbnail_url, "thumbnail_url");

        request
    }


    /// Extract metadata from file content using dedicated extractor
    fn extract_file_metadata(data: &[u8], filename: Option<&str>) -> Option<BlobMetadata> {
        AudioMetadataExtractor::extract(data, filename)
    }

    /// Extract rich metadata from S3 head_object response
    fn extract_blob_metadata(head_result: &aws_sdk_s3::operation::head_object::HeadObjectOutput) -> BlobMetadata {
        let mut metadata = BlobMetadata::default();

        if let Some(s3_metadata) = head_result.metadata() {
            // Audio metadata
            metadata.title = s3_metadata.get("title").map(|s| s.to_string());
            metadata.artist = s3_metadata.get("artist").map(|s| s.to_string());
            metadata.album = s3_metadata.get("album").map(|s| s.to_string());
            metadata.genre = s3_metadata.get("genre").map(|s| s.to_string());
            metadata.year = s3_metadata.get("year").and_then(|s| s.parse().ok());
            metadata.duration = s3_metadata.get("duration").and_then(|s| s.parse().ok());
            metadata.bitrate = s3_metadata.get("bitrate").and_then(|s| s.parse().ok());

            // Visual metadata
            metadata.thumbnail_url = s3_metadata.get("thumbnail_url").map(|s| s.to_string());
            metadata.album_art_url = s3_metadata.get("album_art_url").map(|s| s.to_string());

            // Location metadata
            metadata.latitude = s3_metadata.get("latitude").and_then(|s| s.parse().ok());
            metadata.longitude = s3_metadata.get("longitude").and_then(|s| s.parse().ok());
            metadata.location_name = s3_metadata.get("location_name").map(|s| s.to_string());

            // Technical metadata
            metadata.encoding = s3_metadata.get("encoding").map(|s| s.to_string());
            metadata.sample_rate = s3_metadata.get("sample_rate").and_then(|s| s.parse().ok());
            metadata.channels = s3_metadata.get("channels").and_then(|s| s.parse().ok());

            // Custom attributes (any metadata not in standard fields)
            for (key, value) in s3_metadata {
                if !matches!(key.as_str(), 
                    "filename" | "title" | "artist" | "album" | "genre" | "year" | 
                    "duration" | "bitrate" | "thumbnail_url" | "album_art_url" |
                    "latitude" | "longitude" | "location_name" | "encoding" |
                    "sample_rate" | "channels"
                ) {
                    metadata.custom.insert(key.clone(), value.clone());
                }
            }
        }

        // Set mime_type from content_type
        metadata.mime_type = head_result.content_type().map(|s| s.to_string());

        metadata
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

        // Extract and store rich metadata from file content
        if let Some(extracted_metadata) = Self::extract_file_metadata(&data, filename) {
            request = Self::add_metadata_to_request(request, &extracted_metadata);
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

                    // Extract rich metadata from S3 object metadata
                    let metadata = Self::extract_blob_metadata(&head_result);

                    blobs.push(BlobInfo {
                        key: key.clone(),
                        size_bytes: object.size.unwrap_or(0) as u64,
                        content_type: head_result.content_type.clone(),
                        filename,
                        etag: object.e_tag,
                        last_modified: object.last_modified.map(|dt| dt.secs()),
                        metadata,
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
