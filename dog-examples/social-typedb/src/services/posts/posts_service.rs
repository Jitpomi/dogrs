use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use serde_json::Value;
use crate::typedb::TypeDBState;
use crate::services::SocialParams;
use dog_typedb::TypeDBAdapter;
use super::posts_shared;

pub struct PostsService {
    adapter: TypeDBAdapter,
}

impl PostsService {
    pub fn new(state: Arc<TypeDBState>) -> Self {
        Self {
            adapter: TypeDBAdapter::new(state),
        }
    }
}

#[async_trait]
impl DogService<Value, SocialParams> for PostsService {
    fn capabilities(&self) -> ServiceCapabilities {
        posts_shared::capabilities()
    }

    async fn custom(
        &self,
        _ctx: &TenantContext,
        method: &str,
        data: Option<Value>,
        _params: SocialParams,
    ) -> Result<Value> {
        match method.to_lowercase().as_str() {
            "write" => {
                if let Some(data) = data {
                    self.adapter.write(data).await
                } else {
                    Err(anyhow::anyhow!("Write method requires data"))
                }
            }
            "read" => {
                if let Some(data) = data {
                    self.adapter.read(data).await
                } else {
                    Err(anyhow::anyhow!("Read method requires data with query"))
                }
            }
            _ => Err(anyhow::anyhow!("Unsupported method: {}", method)),
        }
    }
}
