use crate::{bounded, SignedUrlBlobStore};
use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region};
use aws_credential_types::Credentials;
use aws_sdk_s3::{primitives::ByteStream as AwsByteStream, Client};
use std::env;

use crate::{
    BlobError, BlobInfo, BlobMetadata, BlobResult, BlobStore, ByteRange, ByteStream, GetResult,
    ObjectHead, PutResult, StoreCapabilities,
};

/// S3-compatible configuration from environment variables
pub struct S3Config {
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub endpoint_url: String,
}

impl std::fmt::Debug for S3Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Config")
            .field("region", &self.region)
            .field("endpoint_url", &self.endpoint_url)
            .field("credentials", &"[redacted]")
            .finish()
    }
}

impl S3Config {
    pub fn from_env() -> BlobResult<Self> {
        fn get_env(key: &str) -> BlobResult<String> {
            env::var(key)
                .map_err(|_| BlobError::invalid(format!("{} environment variable required", key)))
        }

        Ok(Self {
            region: get_env("RUSTFS_REGION")?,
            access_key_id: get_env("RUSTFS_ACCESS_KEY_ID")?,
            secret_access_key: get_env("RUSTFS_SECRET_ACCESS_KEY")?,
            endpoint_url: get_env("RUSTFS_ENDPOINT_URL")?,
        })
    }
}

/// Generic S3-compatible blob store implementation
#[derive(Clone)]
pub struct S3CompatibleStore {
    client: Client,
    bucket: String,
    max_put_bytes: u64,
    scope: String,
    resources: crate::UploadResources,
}

impl S3CompatibleStore {
    pub async fn new(bucket: String) -> BlobResult<Self> {
        let config = S3Config::from_env()?;
        let scope = format!("{}|{}|{}", config.endpoint_url, config.region, bucket);
        let client = Self::create_client(config).await;
        Ok(Self {
            client,
            bucket,
            scope,
            resources: crate::UploadResources::default(),
            max_put_bytes: 5 * 1024 * 1024 * 1024,
        })
    }

    pub async fn with_config(bucket: String, config: S3Config) -> Self {
        let scope = format!("{}|{}|{}", config.endpoint_url, config.region, bucket);
        let client = Self::create_client(config).await;
        Self {
            client,
            bucket,
            scope,
            resources: crate::UploadResources::default(),
            max_put_bytes: 5 * 1024 * 1024 * 1024,
        }
    }

    async fn create_client(config: S3Config) -> Client {
        let credentials = Credentials::new(
            config.access_key_id,
            config.secret_access_key,
            None,
            None,
            "s3-compatible",
        );

        let aws_config = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(config.region))
            .credentials_provider(credentials)
            .endpoint_url(config.endpoint_url)
            .load()
            .await;

        Client::from_conf(
            aws_sdk_s3::config::Builder::from(&aws_config)
                .force_path_style(true) // Required for S3-compatible services
                .build(),
        )
    }

    pub fn with_resources(mut self, resources: crate::UploadResources) -> Self {
        self.resources = resources;
        self
    }
    /// Bound disk staging even when the store is used without BlobAdapter.
    pub fn with_max_put_bytes(mut self, limit: u64) -> Self {
        self.max_put_bytes = limit.min(5 * 1024 * 1024 * 1024);
        self
    }

    fn format_range(&self, range: &ByteRange) -> String {
        match range.end {
            Some(end) => format!("bytes={}-{}", range.start, end),
            None => format!("bytes={}-", range.start),
        }
    }

    fn resolve_range(
        range: &ByteRange,
        length: u64,
        header: Option<&str>,
    ) -> BlobResult<crate::store::ResolvedRange> {
        let invalid = || BlobError::invalid("invalid S3 Content-Range response");
        let value = header
            .and_then(|v| v.strip_prefix("bytes "))
            .ok_or_else(invalid)?;
        let (bounds, total) = value.split_once('/').ok_or_else(invalid)?;
        let (start, end) = bounds.split_once('-').ok_or_else(invalid)?;
        let (start, end, total): (u64, u64, u64) = (
            start.parse().map_err(|_| invalid())?,
            end.parse().map_err(|_| invalid())?,
            total.parse().map_err(|_| invalid())?,
        );
        if start != range.start
            || start > end
            || end >= total
            || end - start + 1 != length
            || end != range.end.unwrap_or(total - 1).min(total - 1)
        {
            return Err(invalid());
        }
        Ok(crate::store::ResolvedRange {
            start,
            end,
            total_size: total,
        })
    }

    fn map_aws_error(err: impl std::error::Error + Send + Sync + 'static) -> BlobError {
        BlobError::backend(err)
    }

    /// Add metadata fields to S3 put request
    pub fn add_metadata_to_request(
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
        add_optional_metadata!(&metadata.thumbnail_url, "thumbnail_url");
        add_optional_metadata!(&metadata.album_art_url, "album_art_url");
        add_optional_metadata!(metadata.latitude, "latitude", to_string);
        add_optional_metadata!(metadata.longitude, "longitude", to_string);
        add_optional_metadata!(&metadata.location_name, "location_name");

        // Add custom attributes
        for (key, value) in &metadata.custom {
            request = request.metadata(key, value);
        }

        request
    }

    /// Extract rich metadata from S3 head_object response
    pub fn extract_blob_metadata(
        head_result: &aws_sdk_s3::operation::head_object::HeadObjectOutput,
    ) -> BlobMetadata {
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
                if !matches!(
                    key.as_str(),
                    "filename"
                        | "title"
                        | "artist"
                        | "album"
                        | "genre"
                        | "year"
                        | "duration"
                        | "bitrate"
                        | "thumbnail_url"
                        | "album_art_url"
                        | "latitude"
                        | "longitude"
                        | "location_name"
                        | "encoding"
                        | "sample_rate"
                        | "channels"
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
impl BlobStore for S3CompatibleStore {
    fn native_multipart(&self) -> Option<&dyn crate::NativeMultipartStore> {
        Some(self)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn signed_urls(&self) -> Option<&dyn SignedUrlBlobStore> {
        Some(self)
    }
    async fn put(
        &self,
        key: &str,
        content_type: Option<&str>,
        stream: ByteStream,
    ) -> BlobResult<PutResult> {
        self.put_with_metadata(key, content_type, None, stream)
            .await
    }
    async fn put_with_metadata(
        &self,
        key: &str,
        content_type: Option<&str>,
        filename: Option<&str>,
        stream: ByteStream,
    ) -> BlobResult<PutResult> {
        self.resources
            .run(async {
                let staged =
                    bounded::spool_with(stream, self.max_put_bytes, &self.resources).await?;
                self.put_validated(key, content_type, filename, staged)
                    .await
            })
            .await
    }
    async fn put_validated(
        &self,
        key: &str,
        content_type: Option<&str>,
        filename: Option<&str>,
        staged: crate::ValidatedUpload,
    ) -> BlobResult<PutResult> {
        if staged.size_bytes() > self.max_put_bytes {
            return Err(BlobError::invalid("object exceeds store byte limit"));
        }
        let body = AwsByteStream::from_path(staged.path())
            .await
            .map_err(Self::map_aws_error)?;
        let length =
            i64::try_from(staged.size).map_err(|_| BlobError::invalid("object too large"))?;
        let mut request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(body)
            .content_length(length);
        if let Some(ct) = content_type {
            request = request.content_type(ct);
        }
        if let Some(filename) = filename {
            request = request.metadata("filename", filename);
        }
        let result = request.send().await.map_err(Self::map_aws_error)?;
        Ok(PutResult {
            etag: result.e_tag,
            size_bytes: staged.size,
            checksum: Some(staged.checksum.clone()),
        })
    }

    async fn get(&self, key: &str, range: Option<ByteRange>) -> BlobResult<GetResult> {
        let mut request = self.client.get_object().bucket(&self.bucket).key(key);

        if let Some(ref range) = range {
            if range.end.is_some_and(|end| end < range.start) {
                return Err(BlobError::invalid("invalid range"));
            }
            request = request.range(self.format_range(range));
        }

        let result = request.send().await.map_err(Self::map_aws_error)?;
        let content_length = u64::try_from(
            result
                .content_length
                .ok_or_else(|| BlobError::invalid("missing content length"))?,
        )
        .map_err(|_| BlobError::invalid("negative content length"))?;
        let resolved_range = range
            .map(|r| Self::resolve_range(&r, content_length, result.content_range.as_deref()))
            .transpose()?;

        Ok(GetResult {
            stream: Box::pin(async_stream::stream! {
                let mut body = result.body;
                while let Some(chunk) = body.next().await {
                    match chunk {
                        Ok(bytes) => yield Ok(bytes),
                        Err(e) => yield Err(std::io::Error::other(e)),
                    }
                }
            }),
            size_bytes: content_length,
            content_type: result.content_type,
            etag: result.e_tag,
            resolved_range,
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
            .map_err(Self::map_aws_error)?;

        Ok(ObjectHead {
            size_bytes: u64::try_from(
                result
                    .content_length
                    .ok_or_else(|| BlobError::invalid("missing content length"))?,
            )
            .map_err(|_| BlobError::invalid("negative content length"))?,
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
        let mut request = self.client.list_objects_v2().bucket(&self.bucket);

        if let Some(prefix) = prefix {
            request = request.prefix(prefix);
        }

        let limit = limit.unwrap_or(1000);
        if limit == 0 {
            return Ok(Vec::new());
        }
        if limit > 1000 {
            return Err(BlobError::invalid(
                "list limit must not exceed 1000; narrow the prefix",
            ));
        }
        request = request.max_keys(limit as i32);

        let result = request.send().await.map_err(Self::map_aws_error)?;

        let mut blobs = Vec::new();
        if let Some(objects) = result.contents {
            for object in objects {
                if let Some(key) = object.key {
                    // Get additional metadata including filename from head_object
                    let head_result = self
                        .client
                        .head_object()
                        .bucket(&self.bucket)
                        .key(&key)
                        .send()
                        .await
                        .map_err(Self::map_aws_error)?;

                    // Extract filename from metadata if available
                    let filename = head_result
                        .metadata()
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
        StoreCapabilities::basic()
            .with_range()
            .with_signed_urls()
            .with_multipart(Some(5 * 1024 * 1024), Some(5 * 1024 * 1024 * 1024))
    }
}

#[async_trait]
impl SignedUrlBlobStore for S3CompatibleStore {
    async fn sign_get(&self, key: &str, expires: u64) -> BlobResult<String> {
        let config = aws_sdk_s3::presigning::PresigningConfig::expires_in(
            std::time::Duration::from_secs(expires),
        )
        .map_err(Self::map_aws_error)?;
        Ok(self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(config)
            .await
            .map_err(Self::map_aws_error)?
            .uri()
            .to_owned())
    }
    async fn sign_put(
        &self,
        key: &str,
        content_type: Option<&str>,
        expires: u64,
    ) -> BlobResult<String> {
        let config = aws_sdk_s3::presigning::PresigningConfig::expires_in(
            std::time::Duration::from_secs(expires),
        )
        .map_err(Self::map_aws_error)?;
        let mut request = self.client.put_object().bucket(&self.bucket).key(key);
        if let Some(ct) = content_type {
            request = request.content_type(ct);
        }
        Ok(request
            .presigned(config)
            .await
            .map_err(Self::map_aws_error)?
            .uri()
            .to_owned())
    }
}
#[async_trait]
impl crate::NativeMultipartStore for S3CompatibleStore {
    fn maximum_upload_size(&self) -> Option<u64> {
        Some(self.max_put_bytes)
    }
    fn recovery_scope(&self) -> String {
        self.scope.clone()
    }
    fn minimum_part_size(&self) -> u64 {
        5 * 1024 * 1024
    }
    async fn initiate(&self, record: &crate::PendingWrite) -> BlobResult<String> {
        if record.size_bytes > self.max_put_bytes {
            return Err(BlobError::invalid("object exceeds store byte limit"));
        }
        let mut request = self
            .client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(&record.key)
            .metadata("dogrs-write-id", &record.id)
            .metadata("dogrs-sha256", &record.checksum);
        if let Some(ct) = &record.content_type {
            request = request.content_type(ct);
        }
        if let Some(filename) = &record.filename {
            request = request.metadata("filename", filename);
        }
        request
            .send()
            .await
            .map_err(Self::map_aws_error)?
            .upload_id
            .ok_or_else(|| BlobError::upload_failed("missing native upload id"))
    }
    async fn upload_part(
        &self,
        record: &crate::PendingWrite,
        number: u32,
        upload: &crate::ValidatedUpload,
        offset: u64,
        length: u64,
    ) -> BlobResult<String> {
        if number == 0
            || number > 10000
            || length == 0
            || length > 5 * 1024 * 1024 * 1024
            || offset
                .checked_add(length)
                .is_none_or(|end| end > upload.size_bytes())
        {
            return Err(BlobError::invalid("invalid native part bounds"));
        }
        let id = record
            .native_id
            .as_deref()
            .ok_or_else(|| BlobError::invalid("missing native handle"))?;
        let body = AwsByteStream::read_from()
            .path(upload.path())
            .offset(offset)
            .length(aws_sdk_s3::primitives::Length::Exact(length))
            .build()
            .await
            .map_err(Self::map_aws_error)?;
        self.client
            .upload_part()
            .bucket(&self.bucket)
            .key(&record.key)
            .upload_id(id)
            .part_number(number as i32)
            .content_length(length as i64)
            .body(body)
            .send()
            .await
            .map_err(Self::map_aws_error)?
            .e_tag
            .ok_or_else(|| BlobError::upload_failed("missing part etag"))
    }
    async fn finish(
        &self,
        record: &crate::PendingWrite,
        parts: Vec<crate::NativePart>,
    ) -> BlobResult<PutResult> {
        let id = record
            .native_id
            .as_deref()
            .ok_or_else(|| BlobError::invalid("missing native handle"))?;
        let parts = parts
            .into_iter()
            .map(|p| {
                aws_sdk_s3::types::CompletedPart::builder()
                    .part_number(p.number as i32)
                    .e_tag(p.etag)
                    .build()
            })
            .collect();
        let result = self
            .client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(&record.key)
            .upload_id(id)
            .multipart_upload(
                aws_sdk_s3::types::CompletedMultipartUpload::builder()
                    .set_parts(Some(parts))
                    .build(),
            )
            .send()
            .await
            .map_err(Self::map_aws_error)?;
        Ok(PutResult {
            size_bytes: record.size_bytes,
            checksum: Some(record.checksum.clone()),
            etag: result.e_tag,
        })
    }
    async fn inspect(&self, record: &crate::PendingWrite) -> BlobResult<crate::WriteOutcome> {
        use crate::WriteOutcome;
        use aws_sdk_s3::error::ProvideErrorMetadata;
        let absent = match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&record.key)
            .send()
            .await
        {
            Ok(head) => {
                let matching = head.content_length == i64::try_from(record.size_bytes).ok()
                    && head.metadata.as_ref().is_some_and(|m| {
                        m.get("dogrs-write-id") == Some(&record.id)
                            && m.get("dogrs-sha256") == Some(&record.checksum)
                    });
                if matching {
                    return Ok(WriteOutcome::Committed);
                }
                false
            }
            Err(error) if error.as_service_error().is_some_and(|e| e.is_not_found()) => true,
            Err(error) => return Err(Self::map_aws_error(error)),
        };
        let Some(id) = &record.native_id else {
            return Ok(WriteOutcome::Uncertain);
        };
        match self
            .client
            .list_parts()
            .bucket(&self.bucket)
            .key(&record.key)
            .upload_id(id)
            .max_parts(1)
            .send()
            .await
        {
            Err(error)
                if error
                    .as_service_error()
                    .is_some_and(|e| e.code() == Some("NoSuchUpload"))
                    && absent =>
            {
                Ok(WriteOutcome::Aborted)
            }
            Err(error)
                if error
                    .as_service_error()
                    .is_some_and(|e| e.code() == Some("NoSuchUpload")) =>
            {
                Ok(WriteOutcome::Uncertain)
            }
            Err(error) => Err(Self::map_aws_error(error)),
            Ok(_) => Ok(WriteOutcome::Uncertain),
        }
    }
    async fn abort(&self, record: &crate::PendingWrite) -> BlobResult<()> {
        let id = record
            .native_id
            .as_deref()
            .ok_or_else(|| BlobError::invalid("unknown initiation requires provider inventory"))?;
        match self
            .client
            .abort_multipart_upload()
            .bucket(&self.bucket)
            .key(&record.key)
            .upload_id(id)
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(error)
                if error
                    .as_service_error()
                    .is_some_and(|e| e.is_no_such_upload()) =>
            {
                Ok(())
            }
            Err(error) => Err(Self::map_aws_error(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_uses_full_object_size() {
        let r = S3CompatibleStore::resolve_range(
            &ByteRange::from_start(100),
            900,
            Some("bytes 100-999/1000"),
        )
        .unwrap();
        assert_eq!((r.start, r.end, r.total_size), (100, 999, 1000));
        assert!(S3CompatibleStore::resolve_range(
            &ByteRange::from_start(100),
            900,
            Some("bytes 100-899/900")
        )
        .is_err());
        assert!(S3CompatibleStore::resolve_range(&ByteRange::from_start(100), 900, None).is_err());
    }
    #[test]
    fn debug_redacts_credentials() {
        let config = S3Config {
            region: "test".into(),
            endpoint_url: "http://localhost".into(),
            access_key_id: "private-access-marker".into(),
            secret_access_key: "private-secret-marker".into(),
        };
        assert!(!format!("{config:?}").contains("private-"));
    }
}
