use std::sync::Arc;
use tokio::sync::Mutex;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use async_trait::async_trait;
use serde_json::Value;
use crate::services::DemoParams;
use super::devices_shared;

pub struct DevicesService {
    devices: Arc<Mutex<Vec<Value>>>,
}

impl DevicesService {
    pub fn new() -> Self {
        Self {
            devices: Arc::new(Mutex::new(vec![
                serde_json::json!({
                    "id": "device-1",
                    "name": "Living Room Thermostat",
                    "type": "thermostat",
                    "value": 21.5,
                    "status": "online"
                }),
                serde_json::json!({
                    "id": "device-2",
                    "name": "Kitchen Light",
                    "type": "light",
                    "value": "off",
                    "status": "online"
                }),
                serde_json::json!({
                    "id": "device-3",
                    "name": "Front Door Lock",
                    "type": "lock",
                    "value": "locked",
                    "status": "online"
                })
            ])),
        }
    }
}

#[async_trait]
impl DogService<Value, DemoParams> for DevicesService {
    fn capabilities(&self) -> ServiceCapabilities {
        devices_shared::capabilities()
    }

    async fn find(&self, _ctx: &TenantContext, _params: DemoParams) -> anyhow::Result<Vec<Value>> {
        let devs = self.devices.lock().await;
        Ok(devs.clone())
    }

    async fn get(&self, _ctx: &TenantContext, id: &str, _params: DemoParams) -> anyhow::Result<Value> {
        let devs = self.devices.lock().await;
        devs.iter()
            .find(|d| d.get("id").and_then(|v| v.as_str()) == Some(id))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Device not found: {}", id))
    }

    async fn create(&self, _ctx: &TenantContext, data: Value, _params: DemoParams) -> anyhow::Result<Value> {
        let mut devs = self.devices.lock().await;
        let mut new_dev = data;

        let id_val = new_dev.get("id").and_then(|v| v.as_str());
        let final_id = if id_val.is_none() || id_val.unwrap().is_empty() {
            let id_str = uuid::Uuid::new_v4().to_string();
            if let Some(obj) = new_dev.as_object_mut() {
                obj.insert("id".to_string(), serde_json::Value::String(id_str.clone()));
            }
            id_str
        } else {
            id_val.unwrap().to_string()
        };

        if let Some(obj) = new_dev.as_object_mut() {
            if obj.get("status").is_none() {
                obj.insert("status".to_string(), serde_json::Value::String("online".to_string()));
            }
        }

        // Avoid duplicates
        devs.retain(|d| d.get("id").and_then(|v| v.as_str()) != Some(&final_id));
        devs.push(new_dev.clone());
        Ok(new_dev)
    }

    async fn update(
        &self,
        ctx: &TenantContext,
        id: &str,
        data: Value,
        params: DemoParams,
    ) -> anyhow::Result<Value> {
        self.patch(ctx, Some(id), data, params).await
    }

    async fn patch(
        &self,
        _ctx: &TenantContext,
        id: Option<&str>,
        data: Value,
        _params: DemoParams,
    ) -> anyhow::Result<Value> {
        let id = id.ok_or_else(|| anyhow::anyhow!("ID is required for patch"))?;
        let mut devs = self.devices.lock().await;
        let idx = devs.iter().position(|d| d.get("id").and_then(|v| v.as_str()) == Some(id))
            .ok_or_else(|| anyhow::anyhow!("Device not found: {}", id))?;

        let mut current = devs[idx].clone();
        if let (Some(curr_obj), Some(patch_obj)) = (current.as_object_mut(), data.as_object()) {
            for (k, v) in patch_obj {
                if k != "id" {
                    curr_obj.insert(k.clone(), v.clone());
                }
            }
        }
        devs[idx] = current.clone();
        Ok(current)
    }

    async fn remove(
        &self,
        _ctx: &TenantContext,
        id: Option<&str>,
        _params: DemoParams,
    ) -> anyhow::Result<Value> {
        let id = id.ok_or_else(|| anyhow::anyhow!("ID is required for remove"))?;
        let mut devs = self.devices.lock().await;
        let idx = devs.iter().position(|d| d.get("id").and_then(|v| v.as_str()) == Some(id))
            .ok_or_else(|| anyhow::anyhow!("Device not found: {}", id))?;

        let removed = devs.remove(idx);
        Ok(removed)
    }

    async fn custom(
        &self,
        _ctx: &TenantContext,
        method: &str,
        data: Option<Value>,
        _params: DemoParams,
    ) -> anyhow::Result<Value> {
        match method {
            "telemetry" => {
                let payload = data.ok_or_else(|| anyhow::anyhow!("Telemetry requires a payload"))?;
                let device_id = payload.get("id")
                    .or_else(|| payload.get("device_id"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing 'id' or 'device_id' in telemetry payload"))?;

                let mut devs = self.devices.lock().await;
                let idx = devs.iter().position(|d| d.get("id").and_then(|v| v.as_str()) == Some(device_id))
                    .ok_or_else(|| anyhow::anyhow!("Device not found: {}", device_id))?;

                let mut current = devs[idx].clone();
                if let Some(obj) = current.as_object_mut() {
                    if let Some(val) = payload.get("value").or_else(|| payload.get("temperature")) {
                        obj.insert("value".to_string(), val.clone());
                    }
                    if let Some(status) = payload.get("status") {
                        obj.insert("status".to_string(), status.clone());
                    }
                }
                devs[idx] = current.clone();
                Ok(current)
            }
            "toggle" => {
                let payload = data.ok_or_else(|| anyhow::anyhow!("Toggle requires a payload"))?;
                let device_id = payload.get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing 'id' in toggle payload"))?;

                let mut devs = self.devices.lock().await;
                let idx = devs.iter().position(|d| d.get("id").and_then(|v| v.as_str()) == Some(device_id))
                    .ok_or_else(|| anyhow::anyhow!("Device not found: {}", device_id))?;

                let mut current = devs[idx].clone();
                let dev_type = current.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let value = current.get("value").cloned().unwrap_or(serde_json::Value::Null);

                if let Some(obj) = current.as_object_mut() {
                    match dev_type.as_str() {
                        "light" => {
                            let new_val = if value.as_str() == Some("on") { "off" } else { "on" };
                            obj.insert("value".to_string(), serde_json::Value::String(new_val.to_string()));
                        }
                        "lock" => {
                            let new_val = if value.as_str() == Some("locked") { "unlocked" } else { "locked" };
                            obj.insert("value".to_string(), serde_json::Value::String(new_val.to_string()));
                        }
                        _ => return Err(anyhow::anyhow!("Device type '{}' cannot be toggled", dev_type)),
                    }
                }
                devs[idx] = current.clone();
                Ok(current)
            }
            "stats" => {
                let devs = self.devices.lock().await;
                let total = devs.len();
                let online = devs.iter()
                    .filter(|d| d.get("status").and_then(|v| v.as_str()) == Some("online"))
                    .count();

                // Calculate average temperature
                let temps: Vec<f64> = devs.iter()
                    .filter(|d| d.get("type").and_then(|v| v.as_str()) == Some("thermostat"))
                    .filter_map(|d| d.get("value").and_then(|v| v.as_f64()))
                    .collect();

                let avg_temp = if temps.is_empty() {
                    0.0
                } else {
                    temps.iter().sum::<f64>() / (temps.len() as f64)
                };

                Ok(serde_json::json!({
                    "total_devices": total,
                    "online_devices": online,
                    "average_temperature": avg_temp
                }))
            }
            _ => Err(anyhow::anyhow!("Unknown custom method: {}", method))
        }
    }
}
