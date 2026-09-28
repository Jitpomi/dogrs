use crate::{rustfs::RustFsState, services::MusicParams};
use anyhow::Result;
use async_trait::async_trait;
use dog_blob::BlobStore;
use dog_core::hooks::{DogAfterHook, DogBeforeHook, HookContext, HookResult};
use serde_json::Value;
use std::sync::Arc;

pub struct ProcessMulterParams;
#[async_trait]
impl DogBeforeHook<Value, MusicParams> for ProcessMulterParams {
    async fn run(&self, ctx: &mut HookContext<Value, MusicParams>) -> Result<()> {
        ctx.params.cover_art = None;
        if matches!(ctx.method, dog_core::ServiceMethodKind::Custom("upload")) {
            if let Some(data) = &ctx.data {
                let bytes = dog_blob::BlobAdapter::extract_file_data(data).await?;
                ctx.params.cover_art =
                    crate::metadata::audio::AudioMetadataExtractor::extract_raw_album_art(&bytes);
            }
        }
        Ok(())
    }
}
pub struct UploadCoverArtHook {
    pub state: Arc<RustFsState>,
}
#[async_trait]
impl DogAfterHook<Value, MusicParams> for UploadCoverArtHook {
    async fn run(&self, ctx: &mut HookContext<Value, MusicParams>) -> Result<()> {
        if let (Some((mime, bytes)), Some(HookResult::One(result))) =
            (ctx.params.cover_art.take(), &ctx.result)
        {
            if let Some(key) = result.get("key").and_then(Value::as_str) {
                self.state
                    .rustfs_store
                    .put(
                        &format!("{key}_cover"),
                        Some(&mime),
                        Box::pin(futures::stream::once(async move { Ok(bytes.into()) })),
                    )
                    .await?;
            }
        }
        Ok(())
    }
}
