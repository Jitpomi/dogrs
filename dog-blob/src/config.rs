/// Configuration for blob operations
#[derive(Debug, Clone)]
pub struct BlobConfig {
    /// Absolute max size allowed for a single blob (safety guard)
    pub max_blob_bytes: u64,

    /// If size_hint >= this, prefer multipart/resumable path when available
    pub multipart_threshold_bytes: u64,

    /// Rules for part-based uploads
    pub upload_rules: UploadRules,

    /// When a range is requested but store can't do range:
    /// - if false: fall back to full content (HTTP 200 equivalent)
    /// - if true: return Unsupported
    pub require_range_support: bool,

    /// Optional: compute checksums during upload/assembly (streaming)
    pub checksum_alg: Option<String>,
}

impl Default for BlobConfig {
    fn default() -> Self {
        Self {
            max_blob_bytes: 5 * 1024 * 1024 * 1024, // 5GB
            multipart_threshold_bytes: 16 * 1024 * 1024, // 16MB (2x part size)
            upload_rules: UploadRules::default(),
            require_range_support: false,
            checksum_alg: None,
        }
    }
}

/// Rules for multipart uploads
#[derive(Debug, Clone)]
pub struct UploadRules {
    /// Standard part size (bytes). Applies to multipart and staged.
    pub part_size: u64,

    /// Upper bound to protect memory/state
    pub max_parts: u32,

    /// If true: all parts except final must be exactly part_size
    pub require_fixed_part_size: bool,

    /// If true: allow uploading parts in any order
    pub allow_out_of_order: bool,
}

impl Default for UploadRules {
    fn default() -> Self {
        Self {
            part_size: 8 * 1024 * 1024, // 8MB
            max_parts: 10_000,
            require_fixed_part_size: true,
            allow_out_of_order: true,
        }
    }
}

impl BlobConfig {
    /// Create a new config with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Set max blob size
    pub fn with_max_blob_bytes(mut self, bytes: u64) -> Self {
        self.max_blob_bytes = bytes;
        self
    }

    /// Set multipart threshold
    pub fn with_multipart_threshold(mut self, bytes: u64) -> Self {
        self.multipart_threshold_bytes = bytes;
        self
    }

    /// Set upload rules
    pub fn with_upload_rules(mut self, rules: UploadRules) -> Self {
        self.upload_rules = rules;
        self
    }

    /// Require range support (no fallback to full content)
    pub fn require_range_support(mut self) -> Self {
        self.require_range_support = true;
        self
    }

    /// Enable checksum with algorithm
    pub fn with_checksum<S: Into<String>>(mut self, algorithm: S) -> Self {
        self.checksum_alg = Some(algorithm.into());
        self
    }
}

impl UploadRules {
    /// Create new upload rules
    pub fn new() -> Self {
        Self::default()
    }

    /// Set part size
    pub fn with_part_size(mut self, bytes: u64) -> Self {
        self.part_size = bytes;
        self
    }

    /// Set max parts
    pub fn with_max_parts(mut self, max: u32) -> Self {
        self.max_parts = max;
        self
    }

    /// Allow variable part sizes (relaxed mode)
    pub fn allow_variable_part_sizes(mut self) -> Self {
        self.require_fixed_part_size = false;
        self
    }

    /// Require parts to be uploaded in order
    pub fn require_ordered_parts(mut self) -> Self {
        self.allow_out_of_order = false;
        self
    }
}
