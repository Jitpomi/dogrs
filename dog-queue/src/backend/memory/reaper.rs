use std::sync::Arc;
use std::time::Duration;
use chrono::Utc;
use tokio::time::interval;
use tracing::{info, warn, debug};

use crate::{
    JobStatus,
    backend::memory::storage::MemoryBackend,
    QueueResult, JobEvent,
};

/// Lease expiry reaper for reclaiming expired jobs
pub struct LeaseReaper {
    backend: Arc<MemoryBackend>,
    interval: Duration,
}

impl LeaseReaper {
    /// Create a new lease reaper
    pub fn new(backend: Arc<MemoryBackend>) -> Self {
        Self {
            backend,
            interval: Duration::from_secs(30), // Run every 30 seconds
        }
    }

    /// Create reaper with custom interval
    pub fn with_interval(backend: Arc<MemoryBackend>, interval: Duration) -> Self {
        Self { backend, interval }
    }

    /// Start the reaper background task
    pub async fn start(self) -> QueueResult<()> {
        let mut ticker = interval(self.interval);
        
        info!("Starting lease reaper with interval: {:?}", self.interval);
        
        loop {
            ticker.tick().await;
            
            match self.reap_expired_leases().await {
                Ok(reclaimed_count) => {
                    if reclaimed_count > 0 {
                        info!("Reclaimed {} expired leases", reclaimed_count);
                    } else {
                        debug!("No expired leases found");
                    }
                }
                Err(e) => {
                    warn!("Error during lease reaping: {}", e);
                }
            }
        }
    }

    /// Run one reaper cycle (for testing)
    pub async fn reap_expired_leases(&self) -> QueueResult<usize> {
        let now = Utc::now();
        let mut reclaimed_count = 0;

        // Get all jobs with expired leases
        let expired_jobs = {
            let jobs = self.backend.jobs.read();
            jobs.iter()
                .filter_map(|(job_id, record)| {
                    match &record.status {
                        JobStatus::Processing { lease_until } if *lease_until < now => {
                            Some((job_id.clone(), record.clone()))
                        }
                        _ => None,
                    }
                })
                .collect::<Vec<_>>()
        };

        // Reclaim expired jobs
        for (job_id, mut record) in expired_jobs {
            debug!("Reclaiming expired lease for job: {}", job_id);
            
            // Update job status back to retrying or enqueued
            let new_status = if record.attempt >= record.message.max_retries {
                // Max retries exceeded - mark as failed
                JobStatus::Failed {
                    failed_at: now,
                    error: "Max retries exceeded due to lease expiry".to_string(),
                }
            } else {
                // Make immediately available for retry
                JobStatus::Retrying {
                    retry_at: now, // Retry immediately
                }
            };

            // Update record
            record.status = new_status.clone();
            record.lease_token = None;
            record.lease_until = None;
            record.updated_at = now;
            record.set_error("Lease expired".to_string());

            // Store updated record
            self.backend.jobs.write().insert(job_id.clone(), record.clone());

            // Re-add to queue if retrying
            if matches!(new_status, JobStatus::Retrying { .. }) {
                let mut queues = self.backend.queues.write();
                let tenant_queues = queues.entry(record.tenant_id.clone()).or_default();
                let queue = tenant_queues.entry(record.message.queue.clone()).or_default();
                queue.push_back(job_id.clone());
            }

            // Emit appropriate event
            let event = match new_status {
                JobStatus::Retrying { retry_at, .. } => JobEvent::Retrying {
                    job_id: job_id.clone(),
                    retry_at,
                    error: "Lease expired".to_string(),
                    at: now,
                },
                JobStatus::Failed { error, .. } => JobEvent::Failed {
                    job_id: job_id.clone(),
                    error,
                    at: now,
                },
                _ => continue,
            };

            let _ = self.backend.event_broadcaster.send(event);
            reclaimed_count += 1;
        }

        Ok(reclaimed_count)
    }
}

/// Test helpers for deterministic testing
impl MemoryBackend {
    /// Force a lease to expire (test helper)
    pub async fn force_lease_expiry(&self, job_id: crate::JobId) -> QueueResult<()> {
        let mut jobs = self.jobs.write();
        if let Some(record) = jobs.get_mut(&job_id) {
            if let JobStatus::Processing { ref mut lease_until } = record.status {
                *lease_until = Utc::now() - chrono::Duration::seconds(1);
                record.updated_at = Utc::now();
            }
        }
        Ok(())
    }

    /// Run one reaper tick (test helper)
    pub async fn run_reaper_tick(&self) -> QueueResult<()> {
        let reaper = LeaseReaper::new(Arc::new(self.clone()));
        reaper.reap_expired_leases().await?;
        Ok(())
    }

    /// Advance time concept (test helper - for Memory backend, this is a no-op since we use real time)
    pub async fn advance_time_to(&self, _target_time: chrono::DateTime<Utc>) -> QueueResult<()> {
        // For memory backend, we can't actually advance time
        // Tests should use force_lease_expiry or similar helpers
        Ok(())
    }
}

// Need to implement Clone for MemoryBackend to support test helpers
impl Clone for MemoryBackend {
    fn clone(&self) -> Self {
        Self {
            jobs: self.jobs.clone(),
            queues: self.queues.clone(),
            idempotency: self.idempotency.clone(),
            event_broadcaster: self.event_broadcaster.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::QueueBackend;
    use crate::{QueueCtx, JobMessage, JobPriority};

    fn create_test_context() -> QueueCtx {
        QueueCtx::new("test_tenant".to_string())
    }

    fn create_test_job_message() -> JobMessage {
        JobMessage {
            job_type: "test_job".to_string(),
            payload_bytes: b"test_payload".to_vec(),
            codec: "json".to_string(),
            queue: "default".to_string(),
            priority: JobPriority::Normal,
            max_retries: 3,
            run_at: chrono::Utc::now(),
            idempotency_key: None,
        }
    }

    #[tokio::test]
    async fn test_lease_expiry_reaper() {
        let backend = Arc::new(MemoryBackend::new());
        let ctx = create_test_context();
        let message = create_test_job_message();

        // Enqueue and lease a job
        let job_id = backend.enqueue(ctx.clone(), message).await.unwrap();
        let _leased = backend.dequeue(ctx.clone(), &["default"]).await.unwrap().unwrap();

        // Force lease expiry
        backend.force_lease_expiry(job_id.clone()).await.unwrap();

        // Run reaper
        let reaper = LeaseReaper::new(backend.clone());
        let reclaimed = reaper.reap_expired_leases().await.unwrap();

        assert_eq!(reclaimed, 1);

        // Job should be available for dequeue again
        let retry_leased = backend.dequeue(ctx, &["default"]).await.unwrap();
        assert!(retry_leased.is_some());
        assert_eq!(retry_leased.unwrap().record.attempt, 2); // Attempt incremented
    }

    #[tokio::test]
    async fn test_max_retries_exceeded() {
        let backend = Arc::new(MemoryBackend::new());
        let ctx = create_test_context();
        let mut message = create_test_job_message();
        message.max_retries = 1; // Only 1 retry allowed

        // Enqueue and lease a job
        let job_id = backend.enqueue(ctx.clone(), message).await.unwrap();
        let _leased = backend.dequeue(ctx.clone(), &["default"]).await.unwrap().unwrap();

        // Simulate job running for too long (lease expires after max retries)
        {
            let mut jobs = backend.jobs.write();
            if let Some(record) = jobs.get_mut(&job_id) {
                record.attempt = 1; // Already at max retries
            }
        }

        // Force lease expiry
        backend.force_lease_expiry(job_id.clone()).await.unwrap();

        // Run reaper
        let reaper = LeaseReaper::new(backend.clone());
        let reclaimed = reaper.reap_expired_leases().await.unwrap();

        assert_eq!(reclaimed, 1);

        // Job should be marked as failed
        let status = backend.get_status(ctx, job_id).await.unwrap();
        assert!(matches!(status, JobStatus::Failed { .. }));
    }
}
