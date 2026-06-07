use super::vehicles_shared;
use crate::services::FleetParams;
use crate::typedb::TypeDBState;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::errors::{DogError, ErrorKind};
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use dog_typedb::TypeDBAdapter;
use serde_json::Value;
use std::sync::Arc;

pub struct VehiclesService {
    adapter: TypeDBAdapter,
}

impl VehiclesService {
    pub fn new(state: Arc<TypeDBState>) -> Self {
        Self {
            adapter: TypeDBAdapter::new(state),
        }
    }
}

#[async_trait]
impl DogService<Value, FleetParams> for VehiclesService {
    fn capabilities(&self) -> ServiceCapabilities {
        vehicles_shared::capabilities()
    }

    async fn custom(
        &self,
        _ctx: &TenantContext,
        method: &str,
        data: Option<Value>,
        _params: FleetParams,
    ) -> Result<Value> {
        match method {
            "read" => self.adapter.read(data.ok_or_else(|| DogError::new(ErrorKind::BadRequest, "Missing request body".to_string()).into_anyhow())?).await,
            "write" => self.adapter.write(data.ok_or_else(|| DogError::new(ErrorKind::BadRequest, "Missing request body".to_string()).into_anyhow())?).await,
            _ => Err(DogError::new(
                ErrorKind::MethodNotAllowed,
                format!("Unknown method: {}", method),
            )
            .into_anyhow()),
        }
    }
}
