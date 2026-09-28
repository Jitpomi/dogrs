use axum::{body::Body, extract::Request, http::StatusCode, response::Response};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use tower::{Layer, Service};

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
        }
    }
}

/// How to encode file data in the JSON output
#[derive(Clone, Debug, PartialEq)]
pub enum FileEncoding {
    /// Base64 encode file contents (default)
    Base64,
    /// Store file info but not contents (for large files)
    Metadata,
    /// Skip files entirely
    Skip,
}

impl Default for MultipartConfig {
    fn default() -> Self {
        Self {
            max_file_size: Some(100 * 1024 * 1024),  // 100MB
            max_total_size: Some(500 * 1024 * 1024), // 500MB
            allowed_content_types: HashSet::new(),   // Allow all
            file_encoding: FileEncoding::Base64,
            file_fields: HashSet::new(), // Auto-detect
            text_fields: HashSet::new(), // Auto-detect
            include_metadata: true,
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

    /// Set whether to include metadata in output
    pub fn include_metadata(mut self, include: bool) -> Self {
        self.include_metadata = include;
        self
    }
}

/// Middleware that converts multipart/form-data requests to JSON
#[derive(Clone)]
pub struct MultipartToJson {
    config: MultipartConfig,
    slots: std::sync::Arc<tokio::sync::Semaphore>,
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
            slots: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }

    pub fn with_config(config: MultipartConfig) -> Self {
        Self {
            config,
            slots: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }
}

impl<S> Layer<S> for MultipartToJson {
    type Service = MultipartToJsonService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        MultipartToJsonService {
            inner,
            config: self.config.clone(),
            slots: self.slots.clone(),
        }
    }
}

#[derive(Clone)]
pub struct MultipartToJsonService<S> {
    inner: S,
    config: MultipartConfig,
    slots: std::sync::Arc<tokio::sync::Semaphore>,
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
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        let config = self.config.clone();
        let slots = self.slots.clone();

        Box::pin(async move {
            // Check if this is a multipart request
            let content_type = req
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            println!(
                "🔧 MultipartToJson middleware called with content-type: '{}'",
                content_type
            );

            if content_type.starts_with("multipart/form-data") {
                let Ok(_permit) = slots.try_acquire_owned() else {
                    return Ok(Response::builder()
                        .status(StatusCode::TOO_MANY_REQUESTS)
                        .body(Body::from("too many multipart uploads"))
                        .expect("static response"));
                };
                println!(
                    "🔧 MultipartToJson middleware: Converting multipart to JSON with BlobRef"
                );

                match tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    convert_multipart_to_json(req, &config),
                )
                .await
                .unwrap_or_else(|_| Err("multipart read deadline exceeded".into()))
                {
                    Ok(json_req) => {
                        println!("✅ MultipartToJson middleware: Successfully converted to JSON with BlobRef");
                        inner.call(json_req).await
                    }
                    Err(e) => {
                        println!("❌ MultipartToJson middleware: Failed to convert: {}", e);
                        let response = Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(Body::from(format!("Failed to parse multipart data: {}", e)))
                            .unwrap();
                        Ok(response)
                    }
                }
            } else {
                println!("🔧 MultipartToJson middleware: Passing through non-multipart request");
                // Pass through non-multipart requests
                inner.call(req).await
            }
        })
    }
}

async fn convert_multipart_to_json(
    req: Request<Body>,
    config: &MultipartConfig,
) -> Result<Request<Body>, Box<dyn std::error::Error + Send + Sync>> {
    use base64::Engine;
    let boundary = multer::parse_boundary(
        req.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .ok_or("missing content type")?,
    )?;
    let (mut parts, body) = req.into_parts();
    let max_file = config
        .max_file_size
        .unwrap_or(8 * 1024 * 1024)
        .min(8 * 1024 * 1024) as u64;
    let max_total = config
        .max_total_size
        .unwrap_or(9 * 1024 * 1024)
        .min(9 * 1024 * 1024) as u64;
    let constraints = multer::Constraints::new().size_limit(
        multer::SizeLimit::new()
            .whole_stream(max_total)
            .per_field(max_file),
    );
    let mut multipart =
        multer::Multipart::with_constraints(body.into_data_stream(), boundary, constraints);
    let mut json_map = HashMap::new();
    let mut file_metadata = None;
    let mut fields = 0;
    while let Some(field) = multipart.next_field().await? {
        fields += 1;
        if fields > 32 {
            return Err("too many fields".into());
        }
        let name = field.name().ok_or("missing field name")?.to_owned();
        if json_map.contains_key(&name) {
            return Err("duplicate multipart field".into());
        }
        if name == "file" {
            if config.file_encoding != FileEncoding::Base64 {
                return Err(
                    "upload requires base64 compatibility mode; use /uploads for large files"
                        .into(),
                );
            }
            let filename = field.file_name().unwrap_or("upload").to_owned();
            let content_type = field
                .content_type()
                .map(ToString::to_string)
                .unwrap_or_else(|| "application/octet-stream".into());
            if !config.allowed_content_types.is_empty()
                && !config.allowed_content_types.contains(&content_type)
            {
                return Err("unsupported file content type".into());
            }
            let bytes = field.bytes().await?;
            json_map.insert(
                name,
                json!(base64::engine::general_purpose::STANDARD.encode(bytes)),
            );
            file_metadata = Some((filename, content_type));
        } else {
            let bytes = field.bytes().await?;
            if bytes.len() > 4096 {
                return Err("text field too large".into());
            }
            json_map.insert(name, json!(std::str::from_utf8(&bytes)?));
        }
    }
    let (filename, content_type) = file_metadata.ok_or("missing file")?;
    json_map.insert("filename".into(), json!(filename));
    json_map.insert("content_type".into(), json!(content_type));
    let bytes = serde_json::to_vec(&json_map)?;
    parts
        .headers
        .insert("content-type", "application/json".parse()?);
    parts.headers.remove("transfer-encoding");
    parts
        .headers
        .insert("content-length", bytes.len().to_string().parse()?);
    Ok(Request::from_parts(parts, Body::from(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn preserves_routing_and_supplies_accepted_file_data() {
        let req = Request::builder().method("POST").uri("/music?test=1")
            .header("content-type", "multipart/form-data; boundary=demo")
            .body(Body::from("--demo\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--demo--\r\n")).unwrap();
        let req = convert_multipart_to_json(req, &MultipartConfig::default())
            .await
            .unwrap();
        assert_eq!(req.method(), "POST");
        assert_eq!(req.uri(), "/music?test=1");
        let bytes = axum::body::to_bytes(req.into_body(), 1024).await.unwrap();
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            dog_blob::BlobAdapter::extract_file_data(&data)
                .await
                .unwrap(),
            b"hello"
        );
        assert!(data.to_string().find("temp_path").is_none());
    }
}
