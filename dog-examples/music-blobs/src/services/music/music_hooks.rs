use crate::services::MusicParams;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogBeforeHook, HookContext};
use serde_json::Value;
use std::sync::Arc;
use crate::rustfs::RustFsState;

pub struct ProcessMulterParams {
    pub state: Arc<RustFsState>,
}

#[async_trait]
impl DogBeforeHook<Value, MusicParams> for ProcessMulterParams {
    async fn run(&self, ctx: &mut HookContext<Value, MusicParams>) -> Result<()> {
        if let dog_core::ServiceMethodKind::Custom(method_name) = &ctx.method {
            if *method_name == "upload" {
                if let Some(ref mut data) = ctx.data {
                    // Try to read the file data to check for thumbnail
                    if let Ok(file_data) = extract_file_data(data).await {
                        println!("🎵 Hook read file data ({} bytes)", file_data.len());
                        // Extract cover thumbnail
                        if let Some((mime, img_bytes)) = crate::metadata::audio::AudioMetadataExtractor::extract_raw_album_art(&file_data) {
                            println!("🖼️ Hook extracted album art! Mime: {}, {} bytes", mime, img_bytes.len());
                            let key = data.get("key").and_then(|k| k.as_str()).unwrap_or("unknown");
                            let cover_key = format!("{}_cover", key);
                            let bucket = std::env::var("RUSTFS_BUCKET").unwrap_or_else(|_| "music-blobs".to_string());
                            
                            let cover_stream = aws_sdk_s3::primitives::ByteStream::from(img_bytes);
                            
                            let _ = self.state.rustfs_store.client.put_object()
                                .bucket(&bucket)
                                .key(&cover_key)
                                .content_type(mime)
                                .body(cover_stream)
                                .send()
                                .await;
                            
                            println!("✅ Hook uploaded cover art to S3!");
                                
                            // Add album_art_url="true" to the JSON metadata so frontend knows the cover exists
                            if let Some(obj) = data.as_object_mut() {
                                if let Some(metadata) = obj.get_mut("metadata").and_then(|m| m.as_object_mut()) {
                                    metadata.insert("album_art_url".to_string(), serde_json::json!("true"));
                                } else {
                                    let mut meta = serde_json::Map::new();
                                    meta.insert("album_art_url".to_string(), serde_json::json!("true"));
                                    obj.insert("metadata".to_string(), serde_json::Value::Object(meta));
                                }
                                println!("✅ Hook injected album_art_url='true' into metadata");
                            }
                        } else {
                            println!("❌ Hook could not find APIC frame in MP3");
                        }
                    } else {
                        println!("❌ Hook failed to extract file data using temp_path");
                    }
                }
            }
        }
        Ok(())
    }
}

async fn extract_file_data(request_data: &Value) -> Result<Vec<u8>> {
    if let Some(file) = request_data.get("file") {
        if let Some(path) = file.get("temp_path").and_then(|p| p.as_str()) {
            return Ok(tokio::fs::read(path).await?);
        }
    }
    anyhow::bail!("No file found in request")
}
