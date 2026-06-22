// src/backend/nats.rs

#[cfg(feature = "nats")]
pub mod nats {
    use async_trait::async_trait;
    use crate::{
        backend::QueueBackend,
        types::LeaseToken,
        JobMessage, JobId, JobRecord, JobStatus, LeasedJob, QueueCtx, QueueResult, QueueError,
        JobEvent, QueueCapabilities,
    };
    use chrono::{DateTime, Utc};
    use futures_core::Stream;
    use std::pin::Pin;
    use serde::{Deserialize, Serialize};
    use std::{collections::HashMap, sync::Arc};
    use tokio::sync::Mutex;
    use async_nats::{self, ConnectOptions};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct StoredJob {
        message: JobMessage,
        status: JobStatus,
        lease_token: Option<LeaseToken>,
    }

    #[derive(Debug, Clone)]
    pub struct NatsConfig {
        pub url: String,
        pub subject: String,
    }

    #[derive(Debug, Clone)]
    pub struct NatsBackend {
        client: async_nats::Client,
        subject: String,
        jobs: Arc<Mutex<HashMap<JobId, StoredJob>>>,
    }

    impl NatsBackend {
        pub async fn new(config: NatsConfig) -> Result<Self, QueueError> {
            let client = ConnectOptions::new()
                .name("dog_queue_nats_async")
                .connect(&config.url)
                .await
                .map_err(|e| QueueError::BackendUnsupported(format!("NATS async connect error: {}", e)))?;
            Ok(Self {
                client,
                subject: config.subject,
                jobs: Arc::new(Mutex::new(HashMap::new())),
            })
        }

        // Also provide new_async for backward compatibility with the legacy façade.
        pub async fn new_async(config: NatsConfig) -> Result<Self, QueueError> {
            Self::new(config).await
        }
    }

    #[async_trait]
    impl QueueBackend for NatsBackend {
        async fn enqueue(&self, _ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId> {
            let job_id = JobId::new();
            let stored = StoredJob { message: message.clone(), status: JobStatus::Enqueued, lease_token: None };
            {
                let mut jobs = self.jobs.lock().await;
                jobs.insert(job_id.clone(), stored);
            }
            let payload = serde_json::to_string(&message).map_err(|e| QueueError::SerializationError(e.to_string()))?;
            self.client.publish(self.subject.clone(), payload.into()).await.map_err(|e| QueueError::BackendUnsupported(format!("NATS async publish error: {}", e)))?;
            Ok(job_id)
        }
        async fn dequeue(&self, _ctx: QueueCtx, _queues: &[&str]) -> QueueResult<Option<LeasedJob>> {
            Err(QueueError::BackendUnsupported("NATS async dequeue not implemented".into()))
        }
        async fn ack_complete(&self, _ctx: QueueCtx, job_id: JobId, lease_token: LeaseToken, _result_ref: Option<String>) -> QueueResult<()> {
            let mut jobs = self.jobs.lock().await;
            let entry = jobs.get_mut(&job_id).ok_or(QueueError::JobNotFound(job_id.clone()))?;
            if entry.lease_token.as_ref() != Some(&lease_token) { return Err(QueueError::InvalidLeaseToken { job_id }); }
            entry.status = JobStatus::Completed { completed_at: Utc::now() };
            entry.lease_token = None;
            Ok(())
        }
        async fn ack_fail(&self, _ctx: QueueCtx, job_id: JobId, lease_token: LeaseToken, error: String, retry_at: Option<DateTime<Utc>>) -> QueueResult<()> {
            let mut jobs = self.jobs.lock().await;
            let entry = jobs.get_mut(&job_id).ok_or(QueueError::JobNotFound(job_id.clone()))?;
            if entry.lease_token.as_ref() != Some(&lease_token) { return Err(QueueError::InvalidLeaseToken { job_id }); }
            entry.status = JobStatus::Failed { failed_at: Utc::now(), error };
            entry.lease_token = None;
            let _ = retry_at;
            Ok(())
        }
        async fn cancel(&self, _ctx: QueueCtx, job_id: JobId) -> QueueResult<bool> {
            let mut jobs = self.jobs.lock().await;
            if let Some(entry) = jobs.get_mut(&job_id) {
                entry.status = JobStatus::Canceled { canceled_at: Utc::now() };
                entry.lease_token = None;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        async fn get_status(&self, _ctx: QueueCtx, job_id: JobId) -> QueueResult<JobStatus> {
            let jobs = self.jobs.lock().await;
            let entry = jobs.get(&job_id).ok_or(QueueError::JobNotFound(job_id))?;
            Ok(entry.status.clone())
        }
        async fn get_record(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<JobRecord> {
            let jobs = self.jobs.lock().await;
            let entry = jobs.get(&job_id).ok_or(QueueError::JobNotFound(job_id.clone()))?;
            Ok(JobRecord {
                job_id,
                tenant_id: ctx.tenant_id.clone(),
                message: entry.message.clone(),
                status: entry.status.clone(),
                attempt: 1,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                last_error: None,
                result: None,
                lease_token: entry.lease_token.clone(),
            })
        }
        fn event_stream(&self, _ctx: QueueCtx) -> Pin<Box<dyn Stream<Item = JobEvent> + Send>> {
            Box::pin(futures::stream::empty())
        }
        fn capabilities(&self) -> QueueCapabilities { QueueCapabilities::default() }
    }
}
