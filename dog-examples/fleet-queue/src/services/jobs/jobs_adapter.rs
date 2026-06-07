use crate::background::BackgroundSystem;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

pub struct JobsAdapter {
    background_system: Arc<BackgroundSystem>,
}

impl JobsAdapter {
    pub fn new(background_system: Arc<BackgroundSystem>) -> Result<Self> {
        Ok(Self { background_system })
    }

    pub async fn enqueue_job(&self, data: Value) -> Result<Value> {
        let job_type = data
            .get("job_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("job_type is required"))?;

        match job_type {
            "gps_tracking" => {
                let assignment_id = data
                    .get("assignment_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("assignment_id is required for gps_tracking"))?;

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
            "route_rebalancing" => {
                let trigger_reason = data
                    .get("manual_trigger")
                    .and_then(|v| v.as_bool())
                    .map(|is_manual| if is_manual { "manual" } else { "auto" })
                    .unwrap_or("auto")
                    .to_string();

                self.background_system
                    .enqueue_route_rebalancing(vec!["ALL".to_string()], 0, trigger_reason)
                    .await?;

                Ok(serde_json::json!({
                    "status": "enqueued",
                    "job_type": "route_rebalancing",
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
            }
            _ => Err(anyhow::anyhow!("Unknown job type: {}", job_type)),
        }
    }

    pub async fn get_stats(&self) -> Result<Value> {
        self.background_system.get_stats().await
    }
}
