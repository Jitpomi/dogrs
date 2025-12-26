use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use serde_json::Value;
use crate::typedb::TypeDBState;
use crate::services::SocialParams;
use dog_typedb::TypeDBAdapter;
use super::persons_shared;

pub struct PersonsService {
    adapter: TypeDBAdapter,
}

impl PersonsService {
    pub fn new(state: Arc<TypeDBState>) -> Self {
        Self {
            adapter: TypeDBAdapter::new(state.driver.clone(), state.database.clone(), state.operation_mutex.clone()),
        }
    }
}

#[async_trait]
impl DogService<Value, SocialParams> for PersonsService {
    fn capabilities(&self) -> ServiceCapabilities {
        persons_shared::capabilities()
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
            _ => {
                Err(anyhow::anyhow!("Unknown custom method: {}", method))
            }
        }
    }
}
