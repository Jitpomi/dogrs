use anyhow::Result;
use dog_core::DogApp;
use serde_json::Value;
use std::sync::Arc;
use crate::services::FleetParams;
use crate::background::BackgroundSystem;

pub struct JobsAdapter {
    background_system: Arc<BackgroundSystem>,
}

impl JobsAdapter {
    pub fn new(app: &DogApp<Value, FleetParams>) -> Result<Self> {
        // Get the shared background system from app state
        let background_system = app.get::<Arc<BackgroundSystem>>("background_system")
            .ok_or_else(|| anyhow::anyhow!("Background system not found in app state"))?;

        Ok(Self {
            background_system,
        })
    }

    pub async fn enqueue_job(&self, data: Value) -> Result<Value> {
        let job_type = data.get("job_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("job_type is required"))?;

        match job_type {
            "gps_tracking" => {
                let assignment_id = data.get("assignment_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("assignment_id is required for GPS tracking"))?;

                self.background_system
                    .enqueue_gps_tracking(assignment_id.to_string())
                    .await?;

                Ok(serde_json::json!({
                    "status": "enqueued",
                    "job_type": "gps_tracking",
                    "assignment_id": assignment_id,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
            }
            "driver_assignment" | "route_rebalancing" | "sla_monitoring" | 
            "maintenance_scheduling" | "compliance_monitoring" => {
                // These job types are registered but don't have specific enqueue methods yet
                // They can be enqueued directly via the queue adapter when needed
                Ok(serde_json::json!({
                    "status": "acknowledged",
                    "job_type": job_type,
                    "message": "Job type registered but manual enqueue required",
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
            }
            _ => Err(anyhow::anyhow!("Unknown job type: {}", job_type))
        }
    }

    pub async fn get_stats(&self) -> Result<Value> {
        self.background_system.get_stats().await
    }

    pub async fn get_queue_status(&self) -> Result<Value> {
        let stats = self.background_system.get_stats().await?;
        
        Ok(serde_json::json!({
            "queue_status": "active",
            "system_stats": stats,
            "available_job_types": ["gps_tracking"],
            "last_updated": chrono::Utc::now().to_rfc3339()
        }))
    }
}
