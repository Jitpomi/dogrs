use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use dog_core::errors::{DogError, ErrorKind};
use serde_json::Value;
use crate::typedb::TypeDBState;
use crate::services::FleetParams;
use dog_typedb::TypeDBAdapter;
use super::rules_shared;

pub struct RulesService {
    adapter: TypeDBAdapter,
}

impl RulesService {
    pub fn new(state: Arc<TypeDBState>) -> Self {
        Self {
            adapter: TypeDBAdapter::new(state),
        }
    }
}

#[async_trait]
impl DogService<Value, FleetParams> for RulesService {
    fn capabilities(&self) -> ServiceCapabilities {
        rules_shared::capabilities()
    }

    async fn custom(
        &self,
        _ctx: &TenantContext,
        method: &str,
        data: Option<Value>,
        _params: FleetParams,
    ) -> Result<Value> {
        match method {
            "read" => self.adapter.read(data.unwrap()).await,
            "write" => self.adapter.write(data.unwrap()).await,
            _ => Err(DogError::new(ErrorKind::MethodNotAllowed, format!("Unknown method: {}", method)).into_anyhow())
        }
    }
}
