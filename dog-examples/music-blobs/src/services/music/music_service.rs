use super::music_shared;
use crate::rustfs::RustFsState;
use crate::services::adapters::RustFsAdapter;
use crate::services::MusicParams;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::errors::{DogError, ErrorKind};
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use serde_json::Value;
use std::sync::Arc;

pub struct MusicService {
    adapter: RustFsAdapter,
}

impl MusicService {
    pub fn new(state: Arc<RustFsState>) -> Self {
        Self {
            adapter: RustFsAdapter::new(state),
        }
    }
}

#[async_trait]
impl DogService<Value, MusicParams> for MusicService {
    fn capabilities(&self) -> ServiceCapabilities {
        music_shared::capabilities()
    }

    async fn custom(
        &self,
        ctx: &TenantContext,
        method: &str,
        data: Option<Value>,
        _params: MusicParams,
    ) -> Result<Value> {
        let _user_id = &format!("{:?}", ctx.tenant_id);

        match method {
            "upload" => {
                let data = data.ok_or_else(|| anyhow::anyhow!("Upload requires data"))?;
                self.adapter.upload(data).await
            }
            "download" => self.adapter.download(data.unwrap()).await,
            "stream" => self.adapter.stream(data.unwrap()).await,
            "pause" => self.adapter.pause(data.unwrap()).await,
            "resume" => self.adapter.resume(data.unwrap()).await,
            "cancel" => self.adapter.cancel(data.unwrap()).await,
            _ => Err(DogError::new(
                ErrorKind::MethodNotAllowed,
                format!("Unknown method: {}", method),
            )
            .into()),
        }
    }
}
