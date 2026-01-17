use anyhow::Result;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities, DogApp};
use dog_core::errors::{DogError, ErrorKind};
use serde_json::Value;
use crate::services::FleetParams;
use crate::services::tomtom::tomtom_adapter::TomTomAdapter;
use super::tomtom_shared;

pub struct TomTomService {
    adapter: TomTomAdapter,
}

impl TomTomService {
    pub fn new(app: &DogApp<Value, FleetParams>) -> Result<Self> {
        Ok(Self { 
            adapter: TomTomAdapter::new(app)? 
        })
    }
}

#[async_trait]
impl DogService<Value, FleetParams> for TomTomService {
    fn capabilities(&self) -> ServiceCapabilities {
        tomtom_shared::capabilities()
    }

    async fn custom(
        &self,
        _ctx: &TenantContext,
        method: &str,
        data: Option<Value>,
        _params: FleetParams,
    ) -> Result<Value> {
        let data = data.ok_or_else(|| {
            DogError::new(ErrorKind::BadRequest, "Missing request data".to_string())
        })?;

        match method {
            "geocode" => self.adapter.geocode(data).await
                .map_err(|e| DogError::new(ErrorKind::BadRequest, e.to_string()).into_anyhow()),
            "reverse-geocode" => self.adapter.reverse_geocode(data).await
                .map_err(|e| DogError::new(ErrorKind::BadRequest, e.to_string()).into_anyhow()),
            "search" => self.adapter.search_addresses(data).await
                .map_err(|e| DogError::new(ErrorKind::BadRequest, e.to_string()).into_anyhow()),
            "route" => self.adapter.calculate_route(data).await
                .map_err(|e| DogError::new(ErrorKind::BadRequest, e.to_string()).into_anyhow()),
            "eta" => self.adapter.update_eta(data).await
                .map_err(|e| DogError::new(ErrorKind::BadRequest, e.to_string()).into_anyhow()),
            "traffic" => self.adapter.check_traffic(data).await
                .map_err(|e| DogError::new(ErrorKind::BadRequest, e.to_string()).into_anyhow()),
            "stats" => self.adapter.get_stats().await
                .map_err(|e| DogError::new(ErrorKind::BadRequest, e.to_string()).into_anyhow()),
            _ => Err(DogError::new(
                ErrorKind::MethodNotAllowed, 
                format!("Unknown TomTom method: {}", method)
            ).into_anyhow())
        }
    }
}
