use crate::rustfs::RustFsState;
use crate::metadata::MimeDecoder;
use anyhow::Result;
use dog_blob::BlobAdapter;
use serde_json::Value;
use std::sync::Arc;
use futures::StreamExt;
use chrono;
use once_cell::sync::Lazy;
use dashmap::DashMap;
use std::time::{Duration, Instant};

/// Playback session state with production-grade features
#[derive(Debug, Clone)]
pub struct PlaybackSession {
    pub status: PlaybackStatus,
    pub position: u64,
    pub last_updated: chrono::DateTime<chrono::Utc>,
    pub last_activity: Instant,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
    Buffering,
    Error,
}


/// Production-grade session manager with concurrent access and cleanup
pub struct SessionManager {
    sessions: DashMap<String, PlaybackSession>,
    session_timeout: Duration,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
            session_timeout: Duration::from_secs(3600), // 1 hour
        }
    }

    pub fn create_session(&self, key: String, _client_id: Option<String>, _user_agent: Option<String>) -> PlaybackSession {
        let session = PlaybackSession {
            status: PlaybackStatus::Playing,
            position: 0,
            last_updated: chrono::Utc::now(),
            last_activity: Instant::now(),
        };
        
        self.sessions.insert(key, session.clone());
        session
    }

    pub fn update_session_status(&self, key: &str, status: PlaybackStatus, position: Option<u64>) -> Option<PlaybackSession> {
        self.sessions.get_mut(key).map(|mut session| {
            session.status = status;
            session.last_updated = chrono::Utc::now();
            session.last_activity = Instant::now();
            if let Some(pos) = position {
                session.position = pos;
                println!("🎵 Updated session position to: {} seconds", pos);
            }
            session.clone()
        })
    }



    pub fn cleanup_expired_sessions(&self) {
        let now = Instant::now();
        self.sessions.retain(|_, session| {
            now.duration_since(session.last_activity) < self.session_timeout
        });
    }

    pub fn get_active_sessions_count(&self) -> usize {
        self.sessions.len()
    }
}

/// Global session manager instance
static SESSION_MANAGER: Lazy<SessionManager> = Lazy::new(|| {
    let manager = SessionManager::new();
    
    // Start cleanup task
    tokio::spawn(async {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            SESSION_MANAGER.cleanup_expired_sessions();
        }
    });
    
    manager
});

/// RustFsAdapter wraps BlobAdapter and implements music-specific methods
pub struct RustFsAdapter {
    adapter: BlobAdapter,
}

impl RustFsAdapter {
    pub fn new(state: Arc<RustFsState>) -> Self {
        // Create BlobAdapter from the BlobState inside RustFsState
        let adapter = BlobAdapter::new(state.blob_state.clone());

        Self { adapter }
    }


    // Handle multipart form data from Dropzone
    pub async fn upload(&self, data: Value) -> Result<Value> {
        let ctx = Self::create_default_context();

        // Use dog-blob's high-level convenience method
        let result = self.adapter
            .put_from_multipart(ctx, &data)
            .await
            .map_err(|e| anyhow::anyhow!("Upload failed: {}", e))?;

        // Convert result to application response format
        match result {
            dog_blob::ChunkResult::Partial { chunks_received, total_chunks } => {
                let chunk_index = data.get("dzchunkindex")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                    .unwrap_or(0);
                let dzuuid = data.get("dzuuid").and_then(|v| v.as_str());
                
                Ok(serde_json::json!({
                    "status": "chunk_received",
                    "chunk_index": chunk_index,
                    "chunks_received": chunks_received,
                    "total_chunks": total_chunks,
                    "dzuuid": dzuuid,
                    "is_complete": false
                }))
            }
            dog_blob::ChunkResult::Complete { receipt } => {
                let dzuuid = data.get("dzuuid").and_then(|v| v.as_str());
                let total_chunks = data.get("dztotalchunkcount")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
                
                Ok(serde_json::json!({
                    "status": "uploaded",
                    "blob_id": receipt.id.to_string(),
                    "key": receipt.key,
                    "size_bytes": receipt.size_bytes,
                    "content_type": receipt.content_type,
                    "filename": receipt.filename,
                    "created_at": receipt.created_at,
                    "chunk_info": {
                        "dzuuid": dzuuid,
                        "total_chunks": total_chunks,
                        "is_complete": dzuuid.is_some()
                    }
                }))
            }
        }
    }

    pub async fn find(&self, data: Option<Value>) -> Result<Value> {
        let ctx = Self::create_default_context();

        // Extract query parameters from data if provided
        let query = data.as_ref()
            .and_then(|d| d.get("query"))
            .and_then(|q| q.as_str());

        let limit = data.as_ref()
            .and_then(|d| d.get("limit"))
            .and_then(|l| l.as_u64())
            .unwrap_or(50) as usize;

        // Use dog-blob's list method to find uploaded files
        match self.adapter.list(ctx, query, Some(limit)).await {
            Ok(blobs) => {
                let music_files: Vec<serde_json::Value> = blobs
                    .into_iter()
                    // Since files are uploaded through music service which validates audio types,
                    // all blobs in this bucket should be audio files. No need to filter by extension
                    // as keys are UUIDs, not filenames.
                    .map(|blob| Self::serialize_music_file(blob))
                    .collect();

                Ok(serde_json::json!({
                    "status": "success",
                    "count": music_files.len(),
                    "files": music_files
                }))
            }
            Err(e) => {
                Ok(serde_json::json!({
                    "status": "error",
                    "message": format!("Failed to list files: {}", e),
                    "files": []
                }))
            }
        }
    }

    /// Create default blob context (will be extracted from TenantContext in future)
    fn create_default_context() -> dog_blob::BlobCtx {
        dog_blob::BlobCtx::new("default".to_string())
    }


    /// Serialize a blob into music file JSON with MIME decoding
    fn serialize_music_file(blob: dog_blob::BlobInfo) -> serde_json::Value {
        serde_json::json!({
            "key": blob.key,
            "size_bytes": blob.size_bytes,
            "content_type": blob.content_type,
            "filename": MimeDecoder::decode_option(blob.filename.as_ref()),
            "etag": blob.etag,
            "last_modified": blob.last_modified,
            "metadata": {
                "title": MimeDecoder::decode_option(blob.metadata.title.as_ref()),
                "artist": MimeDecoder::decode_option(blob.metadata.artist.as_ref()),
                "album": MimeDecoder::decode_option(blob.metadata.album.as_ref()),
                "genre": blob.metadata.genre,
                "year": blob.metadata.year,
                "duration": blob.metadata.duration,
                "bitrate": blob.metadata.bitrate,
                "thumbnail_url": blob.metadata.thumbnail_url,
                "album_art_url": blob.metadata.album_art_url,
                "latitude": blob.metadata.latitude,
                "longitude": blob.metadata.longitude,
                "location_name": blob.metadata.location_name,
                "mime_type": blob.metadata.mime_type,
                "encoding": blob.metadata.encoding,
                "sample_rate": blob.metadata.sample_rate,
                "channels": blob.metadata.channels,
                "custom": blob.metadata.custom
            }
        })
    }


    pub async fn stream(&self, data: Value) -> Result<Value> {
        let ctx = Self::create_default_context();
        
        // Extract key from request data
        let key = data.get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'key' field in stream request"))?;
            
        println!("🎵 Starting audio stream for key: {}", key);
        
        // Extract blob ID from the key (last part after the last slash)
        let blob_id_str = key.split('/').last().unwrap_or(key);
        let blob_id = dog_blob::BlobId(blob_id_str.to_string());
        
        println!("🎵 Extracted blob ID: {} from key: {}", blob_id_str, key);
        
        // Open the blob and read the actual audio content
        match self.adapter.open(ctx, blob_id, None).await {
            Ok(opened_blob) => {
                println!("🎵 Opened blob - Size: {} bytes", opened_blob.content_length());
                
                // Read the actual audio content using the full key
                let audio_content = self.read_blob_content(key).await?;
                
                // Create a playback session for this stream using the session manager
                let session = SESSION_MANAGER.create_session(
                    key.to_string(),
                    None, // client_id - could be extracted from headers
                    None, // user_agent - could be extracted from headers
                );
                
                // Return the audio content as base64 for the frontend to convert to blob
                use base64::Engine;
                let base64_content = base64::engine::general_purpose::STANDARD.encode(&audio_content);
                
                println!("🎵 Returning {} bytes of audio content as base64", audio_content.len());
                
                Ok(serde_json::json!({
                    "status": "streaming",
                    "key": key,
                    "content_type": "audio/mpeg",
                    "size_bytes": audio_content.len(),
                    "audio_data": base64_content,
                    "session_created": true,
                    "session_status": session.status,
                    "active_sessions": SESSION_MANAGER.get_active_sessions_count()
                }))
            },
            Err(e) => {
                println!("❌ Failed to open blob for streaming: {}", e);
                Err(anyhow::anyhow!("Failed to start audio stream: {}", e))
            }
        }
    }


    // Helper method to read blob content from the actual uploaded files
    async fn read_blob_content(&self, full_key: &str) -> Result<Vec<u8>> {
        println!("🎵 Reading actual blob content for key: {}", full_key);
        
        // Use the RustFS store directly to read the raw MP3 content
        use crate::rustfs_store::RustFSStore;
        use dog_blob::BlobStore;
        
        // Get the bucket name from environment
        let bucket = std::env::var("RUSTFS_BUCKET").unwrap_or_else(|_| "music-blobs".to_string());
        
        // Create a new RustFS store instance to read the content
        let store = RustFSStore::new(bucket).await?;
        
        // Use the full key as provided (e.g., "default/2026/01/uuid")
        println!("🎵 Fetching object with key: {}", full_key);
        
        // Read the actual content from the store
        match store.get(full_key, None).await {
            Ok(get_result) => {
                let mut content = Vec::new();
                let mut stream = get_result.stream;
                
                // Read all chunks from the stream
                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => {
                            content.extend_from_slice(&chunk);
                        },
                        Err(e) => {
                            println!("❌ Error reading chunk: {}", e);
                            return Err(anyhow::anyhow!("Failed to read audio chunk: {}", e));
                        }
                    }
                }
                
                println!("🎵 Successfully read {} bytes of actual MP3 content", content.len());
                Ok(content)
            },
            Err(e) => {
                println!("❌ Failed to read from RustFS store: {}", e);
                Err(anyhow::anyhow!("Failed to read audio content from store: {}", e))
            }
        }
    }

  

    pub async fn pause(&self, data: Value) -> Result<Value> {
        // Extract track information from request data
        let key = data.get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'key' field in pause request"))?;
            
        println!("⏸️ Pausing playback for key: {}", key);
        
        // Extract position from request data if provided
        let position = data.get("position")
            .and_then(|p| p.as_u64());
            
        // Update the playback session status using SessionManager
        if let Some(session) = SESSION_MANAGER.update_session_status(key, PlaybackStatus::Paused, position) {
            println!("✅ Updated session status to paused for key: {}", key);
            
            Ok(serde_json::json!({
                "status": "paused",
                "key": key,
                "message": "Playback paused successfully",
                "session_status": "paused",
                "position": session.position,
                "active_sessions": SESSION_MANAGER.get_active_sessions_count(),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
        } else {
            println!("⚠️ No active session found for key: {}", key);
            
            Ok(serde_json::json!({
                "status": "paused",
                "key": key,
                "message": "No active session found, but pause acknowledged",
                "session_status": "not_found",
                "active_sessions": SESSION_MANAGER.get_active_sessions_count(),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
        }
    }

    pub async fn resume(&self, data: Value) -> Result<Value> {
        // Extract track information from request data
        let key = data.get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'key' field in resume request"))?;
            
        println!("▶️ Resuming playback for key: {}", key);
        
        // Update the playback session status using SessionManager
        if let Some(session) = SESSION_MANAGER.update_session_status(key, PlaybackStatus::Playing, None) {
            println!("✅ Updated session status to playing for key: {}", key);
            
            Ok(serde_json::json!({
                "status": "resumed",
                "key": key,
                "message": "Playback resumed successfully",
                "session_status": "playing",
                "position": session.position,
                "active_sessions": SESSION_MANAGER.get_active_sessions_count(),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
        } else {
            println!("⚠️ No active session found for key: {}", key);
            
            Ok(serde_json::json!({
                "status": "resumed",
                "key": key,
                "message": "No active session found, but resume acknowledged",
                "session_status": "not_found",
                "active_sessions": SESSION_MANAGER.get_active_sessions_count(),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
        }
    }

    pub async fn stop(&self, data: Value) -> Result<Value> {
        // Extract key from request data
        let key = data.get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'key' field in stop request"))?;
            
        println!("⏹️ Stopping operation for key: {}", key);
        
        // Update session status to stopped if it exists
        if let Some(_session) = SESSION_MANAGER.update_session_status(key, PlaybackStatus::Stopped, None) {
            println!("✅ Stopped session for key: {}", key);
            
            Ok(serde_json::json!({
                "status": "stopped",
                "key": key,
                "message": "Operation stopped successfully",
                "session_status": "stopped",
                "active_sessions": SESSION_MANAGER.get_active_sessions_count(),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
        } else {
            // No session found, but acknowledge the stop request
            println!("⚠️ No active session found for key: {}", key);
            Ok(serde_json::json!({
                "status": "stopped",
                "key": key,
                "message": "No active operation found, but stop acknowledged",
                "session_status": "not_found",
                "active_sessions": SESSION_MANAGER.get_active_sessions_count(),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
        }
    }

    pub async fn remove(&self, data: Value) -> Result<Value> {
        let ctx = Self::create_default_context();
        
        // Extract key from request data
        let key = data.get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'key' field in remove request"))?;
            
        println!("🗑️ Deleting blob for key: {}", key);
        
        // Extract blob ID from the key (last part after the last slash)
        let blob_id_str = key.split('/').last().unwrap_or(key);
        let blob_id = dog_blob::BlobId(blob_id_str.to_string());
        
        // Use the adapter's delete method to remove the blob
        match self.adapter.delete(ctx, blob_id).await {
            Ok(_) => {
                println!("✅ Successfully deleted blob: {}", key);
                Ok(serde_json::json!({
                    "status": "deleted",
                    "key": key,
                    "message": "File deleted successfully",
                    "deleted": true,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
            },
            Err(e) => {
                println!("❌ Failed to delete blob: {}", e);
                Err(anyhow::anyhow!("Failed to delete file: {}", e))
            }
        }
    }

    pub async fn cancel(&self, data: Value) -> Result<Value> {
        // Extract key from request data
        let key = data.get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'key' field in cancel request"))?;
            
        println!("⏹️ Stopping operation for key: {}", key);
        
        // Update session status to stopped if it exists
        if let Some(_session) = SESSION_MANAGER.update_session_status(key, PlaybackStatus::Stopped, None) {
            println!("✅ Stopped session for key: {}", key);
            
            Ok(serde_json::json!({
                "status": "stopped",
                "key": key,
                "message": "Operation stopped successfully",
                "session_status": "stopped",
                "active_sessions": SESSION_MANAGER.get_active_sessions_count(),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
        } else {
            // No session found, but acknowledge the stop request
            println!("⚠️ No active session found for key: {}", key);
            Ok(serde_json::json!({
                "status": "stopped",
                "key": key,
                "message": "No active operation found, but stop acknowledged",
                "session_status": "not_found",
                "active_sessions": SESSION_MANAGER.get_active_sessions_count(),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
        }
    }

    pub async fn peaks(&self, data: Value) -> Result<Value> {
        // Extract key from request data
        let key = data.get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'key' field in peaks request"))?;
            
        println!("📊 Generating waveform peaks for key: {}", key);
        
        // Read the audio content
        let audio_content = self.read_blob_content(key).await?;
        
        // Generate peaks from audio content
        let peaks = self.generate_waveform_peaks(&audio_content).await?;
        
        println!("📊 Generated {} peak points for key: {}", peaks.len(), key);
        
        Ok(serde_json::json!({
            "status": "success",
            "key": key,
            "peaks": peaks,
            "cached": false,
            "sample_rate": 44100,
            "duration": 0.0, // Could be extracted from audio metadata if needed
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    /// Generate waveform peaks from audio content using Symphonia for real audio decoding
    async fn generate_waveform_peaks(&self, audio_data: &[u8]) -> Result<Vec<f32>> {

        const PEAK_COUNT: usize = 2000; // Higher resolution for better accuracy
        
        // Try to decode the audio using Symphonia
        match self.decode_audio_with_symphonia(audio_data).await {
            Ok(samples) => {
                println!("📊 Successfully decoded {} audio samples", samples.len());
                Ok(self.calculate_peaks_from_samples(&samples, PEAK_COUNT))
            }
            Err(e) => {
                println!("⚠️ Symphonia decoding failed: {}, falling back to byte analysis", e);
                // Fallback to simplified byte-based analysis
                Ok(self.generate_fallback_peaks(audio_data, PEAK_COUNT))
            }
        }
    }

    /// Decode audio using Symphonia to get real PCM samples
    async fn decode_audio_with_symphonia(&self, audio_data: &[u8]) -> Result<Vec<f32>> {
        use symphonia::core::audio::{AudioBufferRef, Signal};
        use symphonia::core::codecs::DecoderOptions;
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;
        use std::io::Cursor;

        let cursor = Cursor::new(audio_data.to_vec());
        let media_source = MediaSourceStream::new(Box::new(cursor), Default::default());

        let mut hint = Hint::new();
        hint.with_extension("mp3"); // Assume MP3 for now

        let meta_opts = MetadataOptions::default();
        let fmt_opts = FormatOptions::default();

        let probed = symphonia::default::get_probe()
            .format(&hint, media_source, &fmt_opts, &meta_opts)
            .map_err(|e| anyhow::anyhow!("Failed to probe audio format: {}", e))?;

        let mut format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| anyhow::anyhow!("No supported audio tracks found"))?;

        let dec_opts = DecoderOptions::default();
        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &dec_opts)
            .map_err(|e| anyhow::anyhow!("Failed to create decoder: {}", e))?;

        let track_id = track.id;
        let mut samples = Vec::new();

        // Decode packets and collect samples
        loop {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::ResetRequired) => {
                    // Reset decoder and continue
                    decoder.reset();
                    continue;
                }
                Err(symphonia::core::errors::Error::IoError(e)) 
                    if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(anyhow::anyhow!("Format error: {}", e)),
            };

            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(audio_buf) => {
                    // Convert audio buffer to f32 samples
                    match audio_buf {
                        AudioBufferRef::F32(buf) => {
                            // For stereo, take left channel or mix to mono
                            let chan = buf.chan(0);
                            samples.extend_from_slice(chan);
                        }
                        AudioBufferRef::U8(buf) => {
                            let chan = buf.chan(0);
                            samples.extend(chan.iter().map(|&s| (s as f32 - 128.0) / 128.0));
                        }
                        AudioBufferRef::U16(buf) => {
                            let chan = buf.chan(0);
                            samples.extend(chan.iter().map(|&s| (s as f32 - 32768.0) / 32768.0));
                        }
                        AudioBufferRef::U24(buf) => {
                            let chan = buf.chan(0);
                            samples.extend(chan.iter().map(|&s| s.inner() as f32 / 8388608.0));
                        }
                        AudioBufferRef::U32(buf) => {
                            let chan = buf.chan(0);
                            samples.extend(chan.iter().map(|&s| (s as f32 - 2147483648.0) / 2147483648.0));
                        }
                        AudioBufferRef::S8(buf) => {
                            let chan = buf.chan(0);
                            samples.extend(chan.iter().map(|&s| s as f32 / 128.0));
                        }
                        AudioBufferRef::S16(buf) => {
                            let chan = buf.chan(0);
                            samples.extend(chan.iter().map(|&s| s as f32 / 32768.0));
                        }
                        AudioBufferRef::S24(buf) => {
                            let chan = buf.chan(0);
                            samples.extend(chan.iter().map(|&s| s.inner() as f32 / 8388608.0));
                        }
                        AudioBufferRef::S32(buf) => {
                            let chan = buf.chan(0);
                            samples.extend(chan.iter().map(|&s| s as f32 / 2147483648.0));
                        }
                        AudioBufferRef::F64(buf) => {
                            let chan = buf.chan(0);
                            samples.extend(chan.iter().map(|&s| s as f32));
                        }
                    }
                }
                Err(symphonia::core::errors::Error::IoError(e)) 
                    if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(symphonia::core::errors::Error::DecodeError(e)) => {
                    println!("⚠️ Decode error: {}", e);
                    continue;
                }
                Err(e) => return Err(anyhow::anyhow!("Decode error: {}", e)),
            }
        }

        Ok(samples)
    }

    /// Calculate waveform peaks from real audio samples with advanced analysis
    fn calculate_peaks_from_samples(&self, samples: &[f32], peak_count: usize) -> Vec<f32> {
        if samples.is_empty() {
            return vec![0.0; peak_count];
        }

        let mut peaks = Vec::with_capacity(peak_count);
        let samples_per_peak = samples.len() / peak_count;

        // Pre-calculate global statistics for normalization
        let global_peak = samples.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
        
        for i in 0..peak_count {
            let start = i * samples_per_peak;
            let end = std::cmp::min(start + samples_per_peak, samples.len());
            
            if start < samples.len() {
                let chunk = &samples[start..end];
                
                if chunk.is_empty() {
                    peaks.push(0.0);
                    continue;
                }
                
                // 1. RMS (Root Mean Square) - average energy
                let rms = {
                    let sum_squares: f32 = chunk.iter().map(|&s| s * s).sum();
                    (sum_squares / chunk.len() as f32).sqrt()
                };
                
                // 2. Peak amplitude - maximum instantaneous level
                let peak_amp = chunk.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
                
                // 3. Crest factor - dynamic range indicator
                let crest_factor = if rms > 0.0 { peak_amp / rms } else { 1.0 };
                
                // 4. Perceptual loudness weighting (simplified A-weighting approximation)
                let perceptual_weight = self.calculate_perceptual_weight(chunk);
                
                // 5. Zero crossing rate - indicates frequency content
                let zcr = self.calculate_zero_crossing_rate(chunk);
                
                // Advanced combination algorithm
                let base_amplitude = rms * 0.6 + peak_amp * 0.4;
                
                // Apply perceptual weighting
                let perceptual_amplitude = base_amplitude * perceptual_weight;
                
                // Dynamic range enhancement based on crest factor
                let dynamic_factor = 1.0 + (crest_factor - 1.0) * 0.3;
                let enhanced_amplitude = perceptual_amplitude * dynamic_factor;
                
                // Frequency content adjustment
                let freq_factor = 1.0 + zcr * 0.2;
                let final_amplitude = enhanced_amplitude * freq_factor;
                
                // Adaptive normalization based on global context
                let normalized = if global_peak > 0.0 {
                    final_amplitude / global_peak
                } else {
                    final_amplitude
                };
                
                // Apply gentle compression for better visual contrast
                let compressed = self.apply_soft_compression(normalized);
                
                peaks.push(compressed.clamp(0.0, 1.0));
            } else {
                peaks.push(0.0);
            }
        }

        // Post-process for visual enhancement
        self.enhance_visual_contrast(&mut peaks);
        
        println!("📊 Generated {} high-accuracy waveform peaks from {} samples", peaks.len(), samples.len());
        peaks
    }
    
    
    /// Calculate perceptual loudness weighting (simplified A-weighting)
    fn calculate_perceptual_weight(&self, chunk: &[f32]) -> f32 {
        // Simplified perceptual weighting based on signal characteristics
        let avg_amplitude = chunk.iter().map(|&s| s.abs()).sum::<f32>() / chunk.len() as f32;
        
        // Human hearing is most sensitive around 1-4kHz
        // This is a simplified approximation without FFT
        let mid_freq_emphasis = 1.0 + avg_amplitude * 0.3;
        
        // Reduce very low amplitude signals (noise floor)
        if avg_amplitude < 0.01 {
            0.5
        } else {
            mid_freq_emphasis.min(1.5)
        }
    }
    
    /// Calculate zero crossing rate for frequency content estimation
    fn calculate_zero_crossing_rate(&self, chunk: &[f32]) -> f32 {
        if chunk.len() < 2 { return 0.0; }
        
        let mut crossings = 0;
        for i in 1..chunk.len() {
            if (chunk[i-1] >= 0.0) != (chunk[i] >= 0.0) {
                crossings += 1;
            }
        }
        
        (crossings as f32) / (chunk.len() as f32)
    }
    
    /// Apply soft compression for better visual contrast
    fn apply_soft_compression(&self, amplitude: f32) -> f32 {
        // Soft knee compression to enhance quiet details while preserving loud parts
        if amplitude < 0.1 {
            // Boost quiet signals
            amplitude * 2.0
        } else if amplitude > 0.7 {
            // Gentle limiting of loud signals
            0.7 + (amplitude - 0.7) * 0.5
        } else {
            // Linear region
            amplitude
        }
    }
    
    /// Enhance visual contrast for better waveform appearance
    fn enhance_visual_contrast(&self, peaks: &mut [f32]) {
        if peaks.is_empty() { return; }
        
        // Find dynamic range
        let min_val = peaks.iter().fold(f32::INFINITY, |a, &b| a.min(b));
        let max_val = peaks.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
        let range = max_val - min_val;
        
        if range > 0.0 {
            // Normalize to use full dynamic range
            for peak in peaks.iter_mut() {
                *peak = (*peak - min_val) / range;
                
                // Apply subtle S-curve for better contrast
                *peak = self.apply_s_curve(*peak);
            }
        }
    }
    
    /// Apply S-curve for enhanced visual contrast
    fn apply_s_curve(&self, x: f32) -> f32 {
        // Smooth S-curve using tanh approximation
        let enhanced = x * 2.0 - 1.0; // Map to [-1, 1]
        let curved = enhanced.tanh() * 0.8; // Apply curve with gentle limiting
        (curved + 1.0) / 2.0 // Map back to [0, 1]
    }

    /// Fallback peaks generation when Symphonia fails
    fn generate_fallback_peaks(&self, audio_data: &[u8], peak_count: usize) -> Vec<f32> {
        let mut peaks = Vec::with_capacity(peak_count);
        let chunk_size = audio_data.len() / peak_count;
        
        for i in 0..peak_count {
            let start = i * chunk_size;
            let end = std::cmp::min(start + chunk_size, audio_data.len());
            
            if start < audio_data.len() {
                let chunk = &audio_data[start..end];
                let sum_squares: u64 = chunk.iter().map(|&b| {
                    let centered = (b as i16) - 128;
                    (centered * centered) as u64
                }).sum();
                
                let rms = ((sum_squares as f64) / (chunk.len() as f64)).sqrt() / 128.0;
                peaks.push(rms as f32);
            } else {
                peaks.push(0.0);
            }
        }
        
        println!("📊 Generated {} fallback peaks from byte analysis", peaks.len());
        peaks
    }
}
