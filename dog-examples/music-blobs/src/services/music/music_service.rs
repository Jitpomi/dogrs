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

    async fn find(&self, _ctx: &TenantContext, _params: MusicParams) -> Result<Vec<Value>> {
        let result = self.adapter.find(None).await?;
        
        // Extract files array from the adapter response
        if let Some(files) = result.get("files").and_then(|f| f.as_array()) {
            Ok(files.clone())
        } else {
            Ok(vec![])
        }
    }

    async fn remove(&self, _ctx: &TenantContext, id: Option<&str>, _params: MusicParams) -> Result<Value> {
        // Use the id parameter as the key for deletion
        let key = id.ok_or_else(|| anyhow::anyhow!("Missing id for remove operation"))?;
        let data = serde_json::json!({ "key": key });
        self.adapter.remove(data).await
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
            },
            "stream" => self.adapter.stream(data.unwrap()).await,
            "pause" => self.adapter.pause(data.unwrap()).await,
            "resume" => self.adapter.resume(data.unwrap()).await,
            "stop" => self.adapter.stop(data.unwrap()).await,
            "cancel" => self.adapter.cancel(data.unwrap()).await,
            _ => Err(DogError::new(
                ErrorKind::MethodNotAllowed,
                format!("Unknown method: {}", method),
            )
            .into()),
        }
    }
}
