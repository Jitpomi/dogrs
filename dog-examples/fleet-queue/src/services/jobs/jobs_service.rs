use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities, DogApp};
use dog_core::errors::{DogError, ErrorKind};
use serde_json::Value;
use crate::services::FleetParams;
use crate::services::jobs::jobs_adapter::JobsAdapter;
use super::jobs_shared;

pub struct JobsService {
    adapter: JobsAdapter,
}

impl JobsService {
    pub fn new(app: &DogApp<Value, FleetParams>) -> Result<Self> {
        Ok(Self {
            adapter: JobsAdapter::new(app)?,
        })
    }
}

#[async_trait]
impl DogService<Value, FleetParams> for JobsService {
    fn capabilities(&self) -> ServiceCapabilities {
        jobs_shared::capabilities()
    }

    async fn custom(
        &self,
        _ctx: &TenantContext,
        method: &str,
        data: Option<Value>,
        _params: FleetParams,
    ) -> Result<Value> {
        match method {
            "enqueue" => {
                let data = data.ok_or_else(|| {
                    DogError::new(ErrorKind::BadRequest, "Missing job data".to_string())
                })?;

                self.adapter.enqueue_job(data).await
                    .map_err(|e| DogError::new(ErrorKind::GeneralError, e.to_string()).into_anyhow())
            }
            "stats" => {
                self.adapter.get_stats().await
                    .map_err(|e| DogError::new(ErrorKind::GeneralError, e.to_string()).into_anyhow())
            }
            "queue_status" => {
                self.adapter.get_queue_status().await
                    .map_err(|e| DogError::new(ErrorKind::GeneralError, e.to_string()).into_anyhow())
            }
            _ => Err(DogError::new(
                ErrorKind::MethodNotAllowed, 
                format!("Unknown jobs method: {}", method)
            ).into_anyhow())
        }
    }
}
