use super::certifications_shared;
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

pub struct CertificationsService {
    adapter: TypeDBAdapter,
}

impl CertificationsService {
    pub fn new(state: Arc<TypeDBState>) -> Self {
        Self {
            adapter: TypeDBAdapter::new(state),
        }
    }
}

#[async_trait]
impl DogService<Value, FleetParams> for CertificationsService {
    fn capabilities(&self) -> ServiceCapabilities {
        certifications_shared::capabilities()
    }

    async fn custom(
        &self,
        _ctx: &TenantContext,
        method: &str,
        data: Option<Value>,
        _params: FleetParams,
    ) -> Result<Value> {
        match method {
            "read" => {
                self.adapter
                    .read(data.ok_or_else(|| {
                        DogError::new(ErrorKind::BadRequest, "Missing request body".to_string())
                            .into_anyhow()
                    })?)
                    .await
            }
            "write" => {
                self.adapter
                    .write(data.ok_or_else(|| {
                        DogError::new(ErrorKind::BadRequest, "Missing request body".to_string())
                            .into_anyhow()
                    })?)
                    .await
            }
            _ => Err(DogError::new(
                ErrorKind::MethodNotAllowed,
                format!("Unknown method: {}", method),
            )
            .into_anyhow()),
        }
    }
}
