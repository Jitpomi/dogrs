pub mod json;

pub use async_trait::async_trait;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{QueueResult, QueueError, QueueCtx, Job, JobMessage};

/// Trait for job payload codecs
pub trait JobCodec: Send + Sync {
    /// Encode bytes to bytes (for raw payload handling)
    fn encode_bytes(&self, bytes: &[u8]) -> QueueResult<Vec<u8>>;
    
    /// Decode bytes to bytes (for raw payload handling)
    fn decode_bytes(&self, bytes: &[u8]) -> QueueResult<Vec<u8>>;
    
    /// Get codec identifier
    fn codec_id(&self) -> &'static str;
}

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
    
    /// Register a new codec
    pub fn register(&mut self, codec: Arc<dyn JobCodec>) {
        let codec_id = codec.codec_id().to_string();
        self.codecs.insert(codec_id, codec);
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
    
    /// Encode a job into a JobMessage
    pub fn encode_job<J: Job + Serialize>(&self, job: &J, _ctx: &QueueCtx) -> QueueResult<JobMessage> {
        let codec = self.default_codec()?;
        let payload = serde_json::to_vec(job).map_err(|e| QueueError::SerializationError(e.to_string()))?;
        
        Ok(JobMessage {
            job_type: J::JOB_TYPE.to_string(),
            payload_bytes: payload,
            codec: codec.codec_id().to_string(),
            queue: "default".to_string(), // TODO: Get from context or job
            priority: J::PRIORITY,
            max_retries: J::MAX_RETRIES,
            run_at: chrono::Utc::now(), // Default to immediate execution
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
