use axum::{
    extract::Request,
    http::StatusCode,
    response::Response,
    body::Body,
};
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
pub type FieldProcessor = Box<dyn Fn(&mut FieldContext) -> Result<(), Box<dyn std::error::Error + Send + Sync>> + Send + Sync>;

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
            field_processors: HashMap::new(), // Can't clone function pointers
            global_processors: Vec::new(),    // Can't clone function pointers
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
            max_file_size: Some(100 * 1024 * 1024), // 100MB
            max_total_size: Some(500 * 1024 * 1024), // 500MB
            allowed_content_types: HashSet::new(), // Allow all
            file_encoding: FileEncoding::Base64,
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
        F: Fn(&mut FieldContext) -> Result<(), Box<dyn std::error::Error + Send + Sync>> + Send + Sync + 'static,
    {
        self.field_processors.insert(field_name.to_string(), Box::new(processor));
        self
    }
    
    /// Add global processor that runs on all file fields
    pub fn global_processor<F>(mut self, processor: F) -> Self
    where
        F: Fn(&mut FieldContext) -> Result<(), Box<dyn std::error::Error + Send + Sync>> + Send + Sync + 'static,
    {
        self.global_processors.push(Box::new(processor));
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
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();
        let config = self.config.clone();
        
        Box::pin(async move {
            // Check if this is a multipart request
            let content_type = req.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            println!("🔧 MultipartToJson middleware called with content-type: '{}'", content_type);

            if content_type.starts_with("multipart/form-data") {
                println!("🔧 MultipartToJson middleware: Converting multipart to JSON with BlobRef");
                
                match convert_multipart_to_json(req, &config).await {
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

async fn convert_multipart_to_json(req: Request<Body>, config: &MultipartConfig) -> Result<Request<Body>, Box<dyn std::error::Error + Send + Sync>> {
    // Store original headers before extracting multipart data
    let original_headers = req.headers().clone();
    
    // Extract boundary from content-type header
    let content_type = original_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    
    let boundary = content_type
        .split("boundary=")
        .nth(1)
        .ok_or("Missing boundary in multipart content-type")?;
    
    // Use multer instead of Axum's parser for large file support
    let body_stream = req.into_body();
    let body_bytes = axum::body::to_bytes(body_stream, 200 * 1024 * 1024).await // 200MB limit
        .map_err(|e| format!("Failed to read request body: {}", e))?;
    
    let mut multipart = multer::Multipart::new(
        futures::stream::once(async { Ok::<bytes::Bytes, multer::Error>(body_bytes) }),
        boundary
    );
    let mut json_map = HashMap::new();
    
    while let Some(field) = multipart.next_field().await? {
        let name = field.name().unwrap_or("unknown").to_string();
        let content_type = field.content_type().map(|ct| ct.to_string());
        let filename = field.file_name().map(|f| f.to_string());
        
        // Determine if this is a file field
        let is_file_field = if !config.file_fields.is_empty() {
            config.file_fields.contains(&name)
        } else if !config.text_fields.is_empty() {
            !config.text_fields.contains(&name)
        } else {
            // Auto-detect: has filename or content-type suggests file
            filename.is_some() || 
            content_type.as_ref().map_or(false, |ct| !ct.starts_with("text/"))
        };
        
        if is_file_field {
            // Handle file field with BlobRef approach - stream to temp storage
            println!("   Processing file field '{}' with content-type: {:?}", name, content_type);
            
            // Create temp file for streaming
            let temp_id = uuid::Uuid::new_v4();
            let temp_path = format!("/tmp/multipart_{}_{}", name, temp_id);
            
            // Ensure temp directory exists
            if let Some(parent) = std::path::Path::new(&temp_path).parent() {
                tokio::fs::create_dir_all(parent).await
                    .map_err(|e| format!("Failed to create temp dir: {}", e))?;
            }
            
            let mut temp_file = tokio::fs::File::create(&temp_path).await
                .map_err(|e| format!("Failed to create temp file: {}", e))?;
            
            let mut total_size = 0u64;
            let mut stream = field;
            
            // Stream chunks directly to disk - no memory buffering
            while let Some(chunk) = stream.chunk().await.map_err(|e| {
                println!("❌ Failed to read chunk from file field '{}': {}", name, e);
                e
            })? {
                use tokio::io::AsyncWriteExt;
                temp_file.write_all(&chunk).await
                    .map_err(|e| format!("Failed to write chunk: {}", e))?;
                total_size += chunk.len() as u64;
            }
            
            // Flush and close the file
            use tokio::io::AsyncWriteExt;
            temp_file.flush().await
                .map_err(|e| format!("Failed to flush file: {}", e))?;
            drop(temp_file);
            
            println!("   File field '{}' streamed to temp file: {} bytes", name, total_size);
            
            // Check file size limits
            if let Some(max_size) = config.max_file_size {
                if total_size > max_size as u64 {
                    return Err(format!("File '{}' exceeds maximum size of {} bytes", name, max_size).into());
                }
            }
            
            // Create BlobRef instead of storing file data
            let blob_ref = serde_json::json!({
                "key": format!("temp/{}", temp_id),
                "temp_path": temp_path,
                "filename": filename,
                "content_type": content_type,
                "size": total_size
            });
            
            json_map.insert(name.clone(), blob_ref);
            
            // Check content type if restricted
            if !config.allowed_content_types.is_empty() {
                if let Some(ct) = &content_type {
                    if !config.allowed_content_types.contains(ct) {
                        return Err(format!("Content type '{}' not allowed for file '{}'", ct, name).into());
                    }
                }
            }

            println!("   File field '{}': {} bytes -> BlobRef", name, total_size);
        } else {
            // Handle text fields
            let value = field.text().await?;
            json_map.insert(name.clone(), json!(value));
            println!("   Text field '{}': {}", name, value);
        }
    }
    
    // Convert to JSON body
    let json_body = json!(json_map);
    let json_bytes = serde_json::to_vec(&json_body)?;
    
    // Create new request with JSON body and preserve original headers
    let (mut parts, _) = Request::new(Body::empty()).into_parts();
    
    // Copy all original headers
    parts.headers = original_headers;
    
    // Update content-type and content-length for JSON body
    parts.headers.insert("content-type", "application/json".parse().unwrap());
    parts.headers.insert("content-length", json_bytes.len().to_string().parse().unwrap());
    
    let new_body = Body::from(json_bytes);
    let new_req = Request::from_parts(parts, new_body);
    
    println!("   Converted to JSON with {} fields", json_map.len());
    
    Ok(new_req)
}
