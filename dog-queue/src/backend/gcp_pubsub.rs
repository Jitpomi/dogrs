// src/backend/gcp_pubsub.rs

#[cfg(feature = "gcp-pubsub")]
pub mod gcp_pubsub {
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
    use std::time::Duration;
    use google_cloud_pubsub::client::{Publisher, Subscriber};
    use google_cloud_pubsub::model::Message as PubSubMessage;
    use serde::{Deserialize, Serialize};
    use std::{collections::HashMap, sync::Arc};
    use tokio::sync::Mutex;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct StoredJob {
        message: JobMessage,
        status: JobStatus,
        lease_token: Option<LeaseToken>,
    }

    #[derive(Debug, Clone)]
    pub struct GcpPubSubConfig {
        pub project_id: String,
        pub topic: String,
        pub subscription: String,
    }

    #[derive(Debug, Clone)]
    pub struct GcpPubSubBackend {
        publisher: Publisher,
        subscriber: Subscriber,
        subscription_path: String,
        jobs: Arc<Mutex<HashMap<JobId, StoredJob>>>,
    }

    impl GcpPubSubBackend {
        pub async fn new(config: GcpPubSubConfig) -> Result<Self, QueueError> {
            let topic_path = format!("projects/{}/topics/{}", config.project_id, config.topic);
            let subscription_path = format!("projects/{}/subscriptions/{}", config.project_id, config.subscription);
            
            let publisher = Publisher::builder(topic_path).build().await.map_err(|e| {
                QueueError::BackendUnsupported(format!("GCP PubSub publisher builder error: {}", e))
            })?;
            
            let subscriber = Subscriber::builder().build().await.map_err(|e| {
                QueueError::BackendUnsupported(format!("GCP PubSub subscriber builder error: {}", e))
            })?;

            Ok(Self {
                publisher,
                subscriber,
                subscription_path,
                jobs: Arc::new(Mutex::new(HashMap::new())),
            })
        }
    }

    #[async_trait]
    impl QueueBackend for GcpPubSubBackend {
        async fn enqueue(&self, _ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId> {
            let job_id = JobId::new();
            let stored = StoredJob { message: message.clone(), status: JobStatus::Enqueued, lease_token: None };
            {
                let mut jobs = self.jobs.lock().await;
                jobs.insert(job_id.clone(), stored);
            }
            let payload = serde_json::to_string(&message)
                .map_err(|e| QueueError::SerializationError(e.to_string()))?;
            
            let pub_msg = PubSubMessage::new().set_data(payload);
            self.publisher.publish(pub_msg).await.map_err(|e| {
                QueueError::BackendUnsupported(format!("GCP PubSub publish error: {}", e))
            })?;
            Ok(job_id)
        }

        async fn dequeue(&self, ctx: QueueCtx, _queues: &[&str]) -> QueueResult<Option<LeasedJob>> {
            let mut session = self.subscriber.streaming_pull(&self.subscription_path).start();
            let pull_fut = session.next();
            match tokio::time::timeout(Duration::from_millis(50), pull_fut).await {
                Ok(Some(Ok((msg, handler)))) => {
                    let body = msg.data;
                    let job_msg: JobMessage = serde_json::from_slice(&body).map_err(|e| {
                        QueueError::SerializationError(e.to_string())
                    })?;
                    // Find job_id in map.
                    let job_id_opt = {
                        let jobs = self.jobs.lock().await;
                        jobs.iter().find(|(_, v)| v.message == job_msg).map(|(k, _)| k.clone())
                    };
                    if let Some(job_id) = job_id_opt {
                        let lease = LeaseToken::new();
                        let lease_until = Utc::now() + chrono::Duration::seconds(300);
                        {
                            let mut jobs = self.jobs.lock().await;
                            if let Some(entry) = jobs.get_mut(&job_id) {
                                entry.status = JobStatus::Processing { lease_until };
                                entry.lease_token = Some(lease.clone());
                            }
                        }
                        // Acknowledge message using the handler
                        handler.ack();

                        let record = JobRecord {
                            job_id: job_id.clone(),
                            tenant_id: ctx.tenant_id.clone(),
                            message: job_msg,
                            status: JobStatus::Processing { lease_until },
                            attempt: 1,
                            created_at: Utc::now(),
                            updated_at: Utc::now(),
                            last_error: None,
                            result: None,
                            lease_token: Some(lease.clone()),
                        };
                        Ok(Some(LeasedJob { record, lease_token: lease, lease_until }))
                    } else {
                        // Nack the message if not found in map by dropping the handler
                        drop(handler);
                        Ok(None)
                    }
                }
                Ok(Some(Err(e))) => {
                    Err(QueueError::BackendUnsupported(format!("GCP PubSub pull error: {}", e)))
                }
                Ok(None) => Ok(None),
                Err(_) => {
                    // Timeout elapsed, no message available
                    Ok(None)
                }
            }
        }

        async fn ack_complete(&self, _ctx: QueueCtx, job_id: JobId, lease_token: LeaseToken, _result_ref: Option<String>) -> QueueResult<()> {
            let mut jobs = self.jobs.lock().await;
            let entry = jobs.get_mut(&job_id).ok_or(QueueError::JobNotFound(job_id.clone()))?;
            if entry.lease_token.as_ref() != Some(&lease_token) {
                return Err(QueueError::InvalidLeaseToken { job_id });
            }
            entry.status = JobStatus::Completed { completed_at: Utc::now() };
            entry.lease_token = None;
            Ok(())
        }

        async fn ack_fail(&self, _ctx: QueueCtx, job_id: JobId, lease_token: LeaseToken, error: String, retry_at: Option<DateTime<Utc>>) -> QueueResult<()> {
            let mut jobs = self.jobs.lock().await;
            let entry = jobs.get_mut(&job_id).ok_or(QueueError::JobNotFound(job_id.clone()))?;
            if entry.lease_token.as_ref() != Some(&lease_token) {
                return Err(QueueError::InvalidLeaseToken { job_id });
            }
            entry.lease_token = None;
            tracing::error!(job_id = %job_id, error = %error, "Job failed");
            if let Some(at) = retry_at {
                entry.status = JobStatus::Retrying { retry_at: at };
                let now = Utc::now();
                if at > now {
                    let delay = (at - now).to_std().unwrap_or(Duration::from_secs(0));
                    let payload = serde_json::to_string(&entry.message).map_err(|e| QueueError::SerializationError(e.to_string()))?;
                    let publisher = self.publisher.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(delay).await;
                        let pub_msg = PubSubMessage::new().set_data(payload);
                        let _ = publisher.publish(pub_msg).await;
                    });
                }
            } else {
                entry.status = JobStatus::Failed { failed_at: Utc::now(), error };
            }
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
            let entry = jobs.get(&job_id).ok_or(QueueError::JobNotFound(job_id.clone()))?;
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
                last_error: match &entry.status {
                    JobStatus::Failed { error, .. } => Some(error.clone()),
                    _ => None,
                },
                result: None,
                lease_token: entry.lease_token.clone(),
            })
        }

        fn event_stream(&self, _ctx: QueueCtx) -> Pin<Box<dyn Stream<Item = JobEvent> + Send>> {
            Box::pin(futures::stream::empty())
        }

        fn capabilities(&self) -> QueueCapabilities {
            QueueCapabilities::default()
        }
    }
}

