use axum::{body::Body, extract::Request, http::StatusCode, response::Response};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use tower::{Layer, Service};

/// Field processing context passed to hooks
#[derive(Debug)]
pub struct FieldContext {
    pub name: String,
    pub content_type: Option<String>,
    pub filename: Option<String>,
    pub data: Vec<u8>,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Field processor callback type
pub type FieldProcessor = std::sync::Arc<
    dyn Fn(&mut FieldContext) -> Result<(), Box<dyn std::error::Error + Send + Sync>> + Send + Sync,
>;

/// Configuration for multipart to JSON conversion
pub struct MultipartConfig {
    /// Maximum file size in bytes (None = unlimited)
    pub max_file_size: Option<usize>,
    /// Maximum total request size in bytes (None = unlimited)
    pub max_total_size: Option<usize>,
    /// Allowed content types for files (empty = all allowed)
    pub allowed_content_types: HashSet<String>,
    /// How to encode file data in JSON
    pub file_encoding: FileEncoding,
    /// Field names to treat as files (empty = auto-detect)
    pub file_fields: HashSet<String>,
    /// Field names to treat as text (empty = auto-detect)
    pub text_fields: HashSet<String>,
    /// Whether to include field metadata in output
    pub include_metadata: bool,
    /// Field-specific processors
    pub field_processors: HashMap<String, FieldProcessor>,
    /// Global processors that run on all file fields
    pub global_processors: Vec<FieldProcessor>,
}

impl Clone for MultipartConfig {
    fn clone(&self) -> Self {
        Self {
            max_file_size: self.max_file_size,
            max_total_size: self.max_total_size,
            allowed_content_types: self.allowed_content_types.clone(),
            file_encoding: self.file_encoding.clone(),
            file_fields: self.file_fields.clone(),
            text_fields: self.text_fields.clone(),
            include_metadata: self.include_metadata,
            field_processors: self.field_processors.clone(),
            global_processors: self.global_processors.clone(),
        }
    }
}

/// How to encode file data in the JSON output
#[derive(Clone, Debug, PartialEq)]
pub enum FileEncoding {
    /// Request-scoped temporary file (legacy BlobRef shape). Consume it in the handler.
    TempFile,
    /// Base64 encode file contents
    Base64,
    /// Store file info but not contents (for large files)
    Metadata,
    /// Skip files entirely
    Skip,
}

impl Default for MultipartConfig {
    fn default() -> Self {
        Self {
            max_file_size: Some(10 * 1024 * 1024),  // 10 MiB
            max_total_size: Some(10 * 1024 * 1024), // 10 MiB
            allowed_content_types: HashSet::new(),  // Allow all
            file_encoding: FileEncoding::TempFile,
            file_fields: HashSet::new(), // Auto-detect
            text_fields: HashSet::new(), // Auto-detect
            include_metadata: true,
            field_processors: HashMap::new(),
            global_processors: Vec::new(),
        }
    }
}

impl MultipartConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum file size in bytes
    pub fn max_file_size(mut self, size: usize) -> Self {
        self.max_file_size = Some(size);
        self
    }

    /// Set maximum total request size in bytes
    pub fn max_total_size(mut self, size: usize) -> Self {
        self.max_total_size = Some(size);
        self
    }

    /// Add allowed content type for files
    pub fn allow_content_type(mut self, content_type: &str) -> Self {
        self.allowed_content_types.insert(content_type.to_string());
        self
    }

    /// Set file encoding method
    pub fn file_encoding(mut self, encoding: FileEncoding) -> Self {
        self.file_encoding = encoding;
        self
    }

    /// Add field name to treat as file
    pub fn file_field(mut self, field_name: &str) -> Self {
        self.file_fields.insert(field_name.to_string());
        self
    }

    /// Add field name to treat as text
    pub fn text_field(mut self, field_name: &str) -> Self {
        self.text_fields.insert(field_name.to_string());
        self
    }

    /// Set whether to include metadata in output
    pub fn include_metadata(mut self, include: bool) -> Self {
        self.include_metadata = include;
        self
    }

    /// Add custom field processor for specific field names
    pub fn field_processor<F>(mut self, field_name: &str, processor: F) -> Self
    where
        F: Fn(&mut FieldContext) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
            + Send
            + Sync
            + 'static,
    {
        self.field_processors
            .insert(field_name.to_string(), std::sync::Arc::new(processor));
        self
    }

    /// Add global processor that runs on all file fields
    pub fn global_processor<F>(mut self, processor: F) -> Self
    where
        F: Fn(&mut FieldContext) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
            + Send
            + Sync
            + 'static,
    {
        self.global_processors.push(std::sync::Arc::new(processor));
        self
    }
}

/// Middleware that converts multipart/form-data requests to JSON
///
/// This middleware detects multipart requests and converts them to JSON format
/// that can be consumed by dog-core services. Fully configurable with sensible defaults.
#[derive(Clone)]
pub struct MultipartToJson {
    config: MultipartConfig,
}

impl Default for MultipartToJson {
    fn default() -> Self {
        Self::new()
    }
}

impl MultipartToJson {
    pub fn new() -> Self {
        Self {
            config: MultipartConfig::default(),
        }
    }

    pub fn with_config(config: MultipartConfig) -> Self {
        Self { config }
    }
}

impl<S> Layer<S> for MultipartToJson {
    type Service = MultipartToJsonService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        MultipartToJsonService {
            inner,
            config: self.config.clone(),
        }
    }
}

#[derive(Clone)]
pub struct MultipartToJsonService<S> {
    inner: S,
    config: MultipartConfig,
}

impl<S> Service<Request<Body>> for MultipartToJsonService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Response = Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let replacement = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, replacement);
        let config = self.config.clone();
        Box::pin(async move {
            let multipart = req
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| {
                    v.split(';')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .eq_ignore_ascii_case("multipart/form-data")
                });
            if !multipart {
                return inner.call(req).await;
            }
            match tokio::time::timeout(
                std::time::Duration::from_secs(30),
                convert_multipart_to_json(req, &config),
            )
            .await
            .unwrap_or(Err((StatusCode::REQUEST_TIMEOUT, "Upload timed out")))
            {
                Ok((request, _temporary_files)) => inner.call(request).await,
                Err((status, message)) => Ok(Response::builder()
                    .status(status)
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"message":message}).to_string()))
                    .unwrap()),
            }
        })
    }
}

type UploadError = (StatusCode, &'static str);
type ConvertedUpload = (Request<Body>, Vec<tempfile::TempPath>);

async fn convert_multipart_to_json(
    req: Request<Body>,
    config: &MultipartConfig,
) -> Result<ConvertedUpload, UploadError> {
    use base64::Engine;
    use tokio::io::AsyncWriteExt;
    let bad = (StatusCode::BAD_REQUEST, "Invalid multipart data");
    let too_large = (
        StatusCode::PAYLOAD_TOO_LARGE,
        "Multipart size limit exceeded",
    );
    let internal = (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Upload processing failed",
    );
    let (mut parts, body) = req.into_parts();
    let boundary = multer::parse_boundary(
        parts
            .headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .ok_or(bad)?,
    )
    .map_err(|_| bad)?;
    // Even an explicitly unlimited legacy configuration retains a finite safety ceiling.
    let total_limit = config.max_total_size.unwrap_or(200 * 1024 * 1024);
    let constraints = multer::Constraints::new().size_limit(
        multer::SizeLimit::new()
            .whole_stream(total_limit as u64)
            .per_field(config.max_file_size.unwrap_or(total_limit) as u64),
    );
    let mut multipart =
        multer::Multipart::with_constraints(body.into_data_stream(), boundary, constraints);
    let classify = |error: multer::Error| match error {
        multer::Error::FieldSizeExceeded { .. } | multer::Error::StreamSizeExceeded { .. } => {
            too_large
        }
        _ => bad,
    };
    let mut values = serde_json::Map::new();
    let mut files = Vec::new();
    let output_limit = total_limit.saturating_mul(2).saturating_add(64 * 1024);
    let mut output_size = 2usize;
    while let Some(mut field) = multipart.next_field().await.map_err(classify)? {
        if values.len() >= 1024 {
            return Err(too_large);
        }
        let name = field.name().ok_or(bad)?.to_string();
        if values.contains_key(&name) {
            return Err((StatusCode::BAD_REQUEST, "Duplicate multipart field"));
        }
        let filename = field.file_name().map(str::to_owned);
        let content_type = field.content_type().map(ToString::to_string);
        let is_file = if !config.file_fields.is_empty() {
            config.file_fields.contains(&name)
        } else if !config.text_fields.is_empty() {
            !config.text_fields.contains(&name)
        } else {
            filename.is_some()
                || content_type
                    .as_ref()
                    .is_some_and(|ct| !ct.starts_with("text/"))
        };
        if is_file
            && !config.allowed_content_types.is_empty()
            && !content_type
                .as_ref()
                .is_some_and(|ct| config.allowed_content_types.contains(ct))
        {
            return Err((
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "File content type not allowed",
            ));
        }
        let mut data = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(classify)? {
            if data.len().saturating_add(chunk.len()) > config.max_file_size.unwrap_or(total_limit)
            {
                return Err(too_large);
            }
            data.extend_from_slice(&chunk);
        }
        let mut context = FieldContext {
            name: name.clone(),
            filename,
            content_type,
            data,
            metadata: HashMap::new(),
        };
        if is_file {
            for processor in &config.global_processors {
                processor(&mut context).map_err(|_| internal)?;
            }
        }
        if let Some(processor) = config.field_processors.get(&name) {
            processor(&mut context).map_err(|_| internal)?;
        }
        if context.data.len() > config.max_file_size.unwrap_or(total_limit) {
            return Err(too_large);
        }
        let value = if !is_file {
            json!(String::from_utf8(context.data).map_err(|_| bad)?)
        } else {
            let mut value = serde_json::Map::new();
            match config.file_encoding {
                FileEncoding::Skip => {
                    values.insert(name, serde_json::Value::Null);
                    continue;
                }
                FileEncoding::TempFile => {
                    let temp = tempfile::NamedTempFile::new().map_err(|_| internal)?;
                    let (file, path) = temp.into_parts();
                    // Retain ownership before any await so cancellation removes the file.
                    files.push(path);
                    let mut file = tokio::fs::File::from_std(file);
                    file.write_all(&context.data).await.map_err(|_| internal)?;
                    file.flush().await.map_err(|_| internal)?;
                    value.insert(
                        "key".into(),
                        json!(format!("temp/{}", uuid::Uuid::new_v4())),
                    );
                    value.insert(
                        "temp_path".into(),
                        json!(files.last().unwrap().to_string_lossy()),
                    );
                }
                FileEncoding::Base64 => {
                    value.insert(
                        "data".into(),
                        json!(base64::engine::general_purpose::STANDARD.encode(&context.data)),
                    );
                }
                FileEncoding::Metadata => {}
            }
            value.insert("size".into(), json!(context.data.len()));
            if config.include_metadata {
                value.insert("filename".into(), json!(context.filename));
                value.insert("content_type".into(), json!(context.content_type));
                value.insert("metadata".into(), json!(context.metadata));
            }
            serde_json::Value::Object(value)
        };
        output_size = output_size
            .saturating_add(serde_json::to_vec(&value).map_err(|_| internal)?.len())
            .saturating_add(serde_json::to_vec(&name).map_err(|_| internal)?.len())
            .saturating_add(2);
        if output_size > output_limit {
            return Err(too_large);
        }
        values.insert(name, value);
    }
    let bytes = serde_json::to_vec(&values).map_err(|_| internal)?;
    // Base64 and processor output are bounded too; allow base64's 4/3 expansion.
    if bytes.len() > total_limit.saturating_mul(2).saturating_add(64 * 1024) {
        return Err(too_large);
    }
    parts
        .headers
        .insert("content-type", "application/json".parse().unwrap());
    parts
        .headers
        .insert("content-length", bytes.len().to_string().parse().unwrap());
    parts.headers.remove("transfer-encoding");
    Ok((Request::from_parts(parts, Body::from(bytes)), files))
}
