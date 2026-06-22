// src/backend/aws_sqs.rs

#[cfg(feature = "aws-sqs")]
pub mod aws_sqs {
    use async_trait::async_trait;
    use crate::{
        backend::QueueBackend,
        types::LeaseToken,
        JobMessage, JobId, JobRecord, JobStatus, LeasedJob, QueueCtx, QueueResult, QueueError,
        JobEvent, QueueCapabilities,
    };
    use aws_sdk_sqs::{Client, config::{Region, Credentials}, Config};
    use futures_core::Stream;
    use std::{collections::HashMap, sync::Arc, time::Duration};
    use tokio::sync::Mutex;
    use std::pin::Pin;
    use chrono::Utc;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct StoredJob {
        message: JobMessage,
        status: JobStatus,
        lease_token: Option<LeaseToken>,
    }

    #[derive(Debug, Clone)]
    pub struct AwsSqsConfig {
        pub region: String,
        pub access_key: String,
        pub secret_key: String,
        pub queue_url: String,
    }

    #[derive(Debug, Clone)]
    pub struct AwsSqsBackend {
        client: Client,
        queue_url: String,
        jobs: Arc<Mutex<HashMap<JobId, StoredJob>>>,
    }

    impl AwsSqsBackend {
        pub async fn new(config: AwsSqsConfig) -> Result<Self, QueueError> {
            let credentials = Credentials::new(
                config.access_key,
                config.secret_key,
                None,
                None,
                "aws_sqs_backend",
            );
            let region = Region::new(config.region);
            let shared_config = Config::builder()
                .region(region)
                .credentials_provider(credentials)
                .build();
            let client = Client::from_conf(shared_config);
            Ok(Self {
                client,
                queue_url: config.queue_url,
                jobs: Arc::new(Mutex::new(HashMap::new())),
            })
        }
    }

    #[async_trait]
    impl QueueBackend for AwsSqsBackend {
        async fn enqueue(&self, _ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId> {
            let job_id = JobId::new();
            let stored = StoredJob { message: message.clone(), status: JobStatus::Enqueued, lease_token: None };
            {
                let mut jobs = self.jobs.lock().await;
                jobs.insert(job_id.clone(), stored);
            }
            let payload = serde_json::to_string(&message)
                .map_err(|e| QueueError::SerializationError(e.to_string()))?;
            self.client.send_message()
                .queue_url(&self.queue_url)
                .message_body(payload)
                .send()
                .await
                .map_err(|e| QueueError::BackendUnsupported(format!("AWS SQS send error: {}", e)))?;
            Ok(job_id)
        }

        async fn dequeue(&self, ctx: QueueCtx, _queues: &[&str]) -> QueueResult<Option<LeasedJob>> {
            let resp = self.client.receive_message()
                .queue_url(&self.queue_url)
                .max_number_of_messages(1)
                .wait_time_seconds(0)
                .send()
                .await
                .map_err(|e| QueueError::BackendUnsupported(format!("AWS SQS receive error: {}", e)))?;
            if let Some(messages) = resp.messages {
                if let Some(msg) = messages.into_iter().next() {
                    let body = msg.body.ok_or_else(|| QueueError::BackendUnsupported("Message body missing".into()))?;
                    let job_msg: JobMessage = serde_json::from_str(&body)
                        .map_err(|e| QueueError::SerializationError(e.to_string()))?;
                    // Find JobId in in‑memory map.
                    let job_id_opt = {
                        let jobs = self.jobs.lock().await;
                        jobs.iter().find(|(_, v)| v.message == job_msg).map(|(k, _)| k.clone())
                    };
                    if let Some(job_id) = job_id_opt {
                        // Delete the message from the queue.
                        if let Some(receipt) = msg.receipt_handle {
                            let _ = self.client.delete_message()
                                .queue_url(&self.queue_url)
                                .receipt_handle(receipt)
                                .send()
                                .await;
                        }
                        let lease = LeaseToken::new();
                        let lease_until = Utc::now() + chrono::Duration::seconds(300);
                        {
                            let mut jobs = self.jobs.lock().await;
                            if let Some(entry) = jobs.get_mut(&job_id) {
                                entry.status = JobStatus::Processing { lease_until };
                                entry.lease_token = Some(lease.clone());
                            }
                        }
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
                        Ok(None)
                    }
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
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

        async fn ack_fail(&self, _ctx: QueueCtx, job_id: JobId, lease_token: LeaseToken, error: String, retry_at: Option<chrono::DateTime<chrono::Utc>>) -> QueueResult<()> {
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
                    let payload = serde_json::to_string(&entry.message)
                        .map_err(|e| QueueError::SerializationError(e.to_string()))?;
                    let client = self.client.clone();
                    let url = self.queue_url.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(delay).await;
                        let _ = client.send_message()
                            .queue_url(&url)
                            .message_body(payload)
                            .send()
                            .await;
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

