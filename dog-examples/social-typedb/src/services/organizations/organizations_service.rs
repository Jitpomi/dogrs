use crate::services::SocialParams;
use crate::typedb::TypeDBState;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::errors::{DogError, ErrorKind};
use dog_core::tenant::TenantContext;
use dog_core::ServiceMethodKind;
use dog_core::{DogService, ServiceCapabilities};
use dog_typedb::TypeDBAdapter;
use serde_json::Value;
use std::sync::Arc;

pub struct OrganizationsService {
    adapter: TypeDBAdapter,
}

impl OrganizationsService {
    pub fn new(state: Arc<TypeDBState>) -> Self {
        Self {
            adapter: TypeDBAdapter::new(state),
        }
    }
}

#[async_trait]
impl DogService<Value, SocialParams> for OrganizationsService {
    fn capabilities(&self) -> ServiceCapabilities {
        ServiceCapabilities::from_methods(vec![
            ServiceMethodKind::Custom("read"),
            ServiceMethodKind::Custom("write"),
        ])
    }

    async fn custom(
        &self,
        _ctx: &TenantContext,
        method: &str,
        data: Option<Value>,
        _params: SocialParams,
    ) -> Result<Value> {
        match method {
            "read" => self.adapter.read(data.unwrap()).await,
            "write" => self.adapter.write(data.unwrap()).await,
            _ => Err(DogError::new(
                ErrorKind::MethodNotAllowed,
                format!("Unknown method: {}", method),
            )
            .into_anyhow()),
        }
    }
}
