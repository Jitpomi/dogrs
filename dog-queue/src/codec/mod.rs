pub mod json;


use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{Job, JobMessage, QueueCtx, QueueError, QueueResult};

// ---------------------------------------------------------------------------
// JobCodec trait
// ---------------------------------------------------------------------------

/// Trait for job payload codecs
pub trait JobCodec: Send + Sync {
    /// Encode bytes to bytes (for raw payload handling)
    fn encode_bytes(&self, bytes: &[u8]) -> QueueResult<Vec<u8>>;

    /// Decode bytes to bytes (for raw payload handling)
    fn decode_bytes(&self, bytes: &[u8]) -> QueueResult<Vec<u8>>;

    /// Get codec identifier
    fn codec_id(&self) -> &'static str;
}

// ---------------------------------------------------------------------------
// EnqueueOptions — caller-supplied overrides for encode_job
// ---------------------------------------------------------------------------

/// Optional per-enqueue overrides.
///
/// Both fields are `None` by default:
/// - `queue` defaults to `J::JOB_TYPE` (each job type routes to its own queue).
/// - `run_at` defaults to `Utc::now()` (immediate execution).
///
/// Use `QueueAdapter::enqueue_opts` to pass non-default values.
#[derive(Debug, Clone, Default)]
pub struct EnqueueOptions {
    /// Target queue name. `None` means "use the job-type name as the queue".
    pub queue: Option<String>,

    /// Earliest time the job is eligible for processing. `None` means "run
    /// immediately". Useful for delayed or scheduled jobs.
    pub run_at: Option<DateTime<Utc>>,
}

impl EnqueueOptions {
    /// Immediate execution in the job-type's default queue (the common case).
    pub fn immediate() -> Self {
        Self::default()
    }

    /// Schedule the job to run no earlier than `run_at`.
    pub fn scheduled(run_at: DateTime<Utc>) -> Self {
        Self {
            run_at: Some(run_at),
            ..Default::default()
        }
    }

    /// Route the job to a specific named queue.
    pub fn with_queue(queue: impl Into<String>) -> Self {
        Self {
            queue: Some(queue.into()),
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// CodecRegistry
// ---------------------------------------------------------------------------

/// Registry for managing different codecs
pub struct CodecRegistry {
    codecs: HashMap<String, Arc<dyn JobCodec>>,
    default_codec: String,
}

impl CodecRegistry {
    /// Create a new codec registry with JSON as default
    pub fn new() -> Self {
        let mut registry = Self {
            codecs: HashMap::new(),
            default_codec: "json".to_string(),
        };

        // Register JSON codec as default
        registry.register(Arc::new(crate::codec::json::JsonCodec));
        registry
    }

    /// Register a new codec, returning the previously-registered codec for the same
    /// `codec_id` if one existed.
    ///
    /// The return value allows callers to detect silent overwrites:
    ///
    /// ```no_run
    /// # use dog_queue::codec::{CodecRegistry, JobCodec};
    /// # use std::sync::Arc;
    /// // if register() returns Some, a codec with the same ID was replaced.
    /// // In-flight jobs encoded with the old codec may fail to decode.
    /// # let mut registry = CodecRegistry::new();
    /// # let my_codec: Arc<dyn JobCodec> = Arc::new(dog_queue::JsonCodec);
    /// if let Some(prev) = registry.register(my_codec) {
    ///     eprintln!("Replaced codec '{}' — in-flight jobs may fail", prev.codec_id());
    /// }
    /// ```
    pub fn register(&mut self, codec: Arc<dyn JobCodec>) -> Option<Arc<dyn JobCodec>> {
        let codec_id = codec.codec_id().to_string();
        self.codecs.insert(codec_id, codec)
    }

    /// Get a codec by ID
    pub fn get_codec(&self, codec_id: &str) -> QueueResult<Arc<dyn JobCodec>> {
        self.codecs
            .get(codec_id)
            .cloned()
            .ok_or_else(|| QueueError::CodecNotFound(codec_id.to_string()))
    }

    /// Get the default codec
    pub fn default_codec(&self) -> QueueResult<Arc<dyn JobCodec>> {
        self.get_codec(&self.default_codec)
    }

    /// Set the default codec
    pub fn set_default_codec(&mut self, codec_id: &str) -> QueueResult<()> {
        if self.codecs.contains_key(codec_id) {
            self.default_codec = codec_id.to_string();
            Ok(())
        } else {
            Err(QueueError::CodecNotFound(codec_id.to_string()))
        }
    }

    /// List available codecs
    pub fn available_codecs(&self) -> Vec<String> {
        self.codecs.keys().cloned().collect()
    }

    /// Encode a job into a `JobMessage`, respecting caller-supplied options.
    ///
    /// - `opts.queue`: if `None`, defaults to `J::JOB_TYPE` (each job type gets
    ///   its own queue). Pass a name explicitly to support multi-queue routing or
    ///   priority lanes (e.g. `"email-high"` vs `"email-low"`).
    /// - `opts.run_at`: if `None`, defaults to `Utc::now()` (run immediately).
    ///   Set this to schedule delayed jobs without constructing `JobMessage` manually.
    pub fn encode_job<J: Job + Serialize>(
        &self,
        job: &J,
        _ctx: &QueueCtx,
        opts: EnqueueOptions,
    ) -> QueueResult<JobMessage> {
        let codec = self.default_codec()?;

        // Serialize the job to raw JSON bytes.
        // Use QueueError::from (the From<serde_json::Error> impl) so the error
        // carries the category prefix ("[Syntax]", "[Data]", etc.) for diagnosability.
        let raw = serde_json::to_vec(job).map_err(QueueError::from)?;

        // Pass through the codec's encode_bytes so that custom codecs (compression,
        // encryption, alternate wire formats) are actually applied.
        // Previously this called serde_json::to_vec and discarded the codec object,
        // meaning any registered non-JSON codec was silently bypassed at encode time
        // while still being called at decode time — producing corrupt payloads.
        let payload = codec.encode_bytes(&raw)?;

        Ok(JobMessage {
            job_type: J::JOB_TYPE.to_string(),
            payload_bytes: payload,
            codec: codec.codec_id().to_string(),
            queue: opts.queue.unwrap_or_else(|| J::JOB_TYPE.to_string()),
            priority: J::PRIORITY,
            max_retries: J::MAX_RETRIES,
            run_at: opts.run_at.unwrap_or_else(Utc::now),
            idempotency_key: job.idempotency_key(),
        })
    }

    /// Decode a JobMessage payload
    pub fn decode_job_payload(&self, message: &JobMessage) -> QueueResult<Vec<u8>> {
        let codec = self.get_codec(&message.codec)?;
        codec.decode_bytes(&message.payload_bytes)
    }
}

impl Default for CodecRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for CodecRegistry {
    fn clone(&self) -> Self {
        Self {
            codecs: self.codecs.clone(),
            default_codec: self.default_codec.clone(),
        }
    }
}
