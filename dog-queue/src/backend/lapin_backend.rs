// src/backend/lapin_backend.rs

#[cfg(feature = "lapin")]
pub mod lapin_backend {
    use async_trait::async_trait;
    use crate::{
        backend::QueueBackend,
        types::LeaseToken,
        JobMessage, JobId, JobRecord, JobStatus, LeasedJob, QueueCtx, QueueResult, QueueError,
        JobEvent, QueueCapabilities,
    };
    use chrono::{DateTime, Utc};
    use futures_core::Stream;
    use futures_util::stream::StreamExt;
    use lapin::{
        options::{BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions},
        types::FieldTable,
        BasicProperties, Channel, Connection, ConnectionProperties, Consumer,
    };
    use serde::{Deserialize, Serialize};
    use std::{collections::HashMap, sync::Arc};
    use std::pin::Pin;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct StoredJob {
        message: JobMessage,
        status: JobStatus,
        lease_token: Option<LeaseToken>,
    }

    #[derive(Debug, Clone)]
    pub struct LapinBackend {
        channel: Channel,
        queue_name: String,
        exchange: String,
        routing_key: String,
        jobs: Arc<Mutex<HashMap<JobId, StoredJob>>>,
    }

    impl LapinBackend {
        pub async fn new(config: super::RabbitMqConfig) -> Result<Self, QueueError> {
            let conn = Connection::connect(&config.uri, ConnectionProperties::default())
                .await
                .map_err(|e| QueueError::BackendUnsupported(format!("RabbitMQ connect error: {}", e)))?;
            let channel = conn.create_channel().await.map_err(|e| {
                QueueError::BackendUnsupported(format!("RabbitMQ channel error: {}", e))
            })?;

            let exchange_name = config.exchange.clone().unwrap_or_default();
            if !exchange_name.is_empty() {
                channel.exchange_declare(
                    &exchange_name,
                    lapin::ExchangeKind::Direct,
                    lapin::options::ExchangeDeclareOptions::default(),
                    FieldTable::default(),
                ).await.map_err(|e| {
                    QueueError::BackendUnsupported(format!("RabbitMQ exchange declare error: {}", e))
                })?;
            }

            channel.queue_declare(
                &config.queue_name,
                QueueDeclareOptions {
                    durable: config.durable,
                    ..Default::default()
                },
                config.arguments.clone(),
            ).await.map_err(|e| {
                QueueError::BackendUnsupported(format!("RabbitMQ queue declare error: {}", e))
            })?;

            if !exchange_name.is_empty() {
                let routing = config.routing_key.clone().unwrap_or_else(|| config.queue_name.clone());
                channel.queue_bind(
                    &config.queue_name,
                    &exchange_name,
                    &routing,
                    lapin::options::QueueBindOptions::default(),
                    FieldTable::default(),
                ).await.map_err(|e| {
                    QueueError::BackendUnsupported(format!("RabbitMQ bind error: {}", e))
                })?;
            }

            if let Some(pref) = config.prefetch {
                channel.basic_qos(pref, 0, true).await.map_err(|e| {
                    QueueError::BackendUnsupported(format!("RabbitMQ qos error: {}", e))
                })?;
            }

            Ok(Self {
                channel,
                queue_name: config.queue_name,
                exchange: exchange_name,
                routing_key: config.routing_key.unwrap_or_default(),
                jobs: Arc::new(Mutex::new(HashMap::new())),
            })
        }
    }

    #[async_trait]
    impl QueueBackend for LapinBackend {
        async fn enqueue(&self, _ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId> {
            let job_id = JobId::new();
            let stored = StoredJob { message: message.clone(), status: JobStatus::Ready, lease_token: None };
            {
                let mut jobs = self.jobs.lock().await;
                jobs.insert(job_id.clone(), stored);
            }
            let payload = serde_json::to_string(&message)
                .map_err(|e| QueueError::SerializationError(e.to_string()))?;
            let exchange = if self.exchange.is_empty() { "" } else { &self.exchange };
            let routing = if self.routing_key.is_empty() { &self.queue_name } else { &self.routing_key };
            self.channel
                .basic_publish(
                    exchange,
                    routing,
                    BasicPublishOptions::default(),
                    payload.into_bytes(),
                    BasicProperties::default().with_content_type("application/json".into()),
                )
                .await
                .map_err(|e| QueueError::BackendUnsupported(format!("RabbitMQ publish error: {}", e)))?;
            Ok(job_id)
        }

        async fn dequeue(&self, _ctx: QueueCtx, _queues: &[&str]) -> QueueResult<Option<LeasedJob>> {
            let mut consumer: Consumer = self.channel.basic_consume(
                &self.queue_name,
                "dog_queue_consumer",
                BasicConsumeOptions::default(),
                FieldTable::default(),
            ).await.map_err(|e| QueueError::BackendUnsupported(format!("RabbitMQ consume error: {}", e)))?;

            if let Some(delivery) = consumer.next().await {
                let delivery = delivery.map_err(|e| QueueError::BackendUnsupported(format!("RabbitMQ delivery error: {}", e)))?;
                let job_msg: JobMessage = serde_json::from_slice(&delivery.data)
                    .map_err(|e| QueueError::SerializationError(e.to_string()))?;
                let job_id = JobId::new();
                let lease = LeaseToken::new();
                {
                    let mut jobs = self.jobs.lock().await;
                    jobs.insert(job_id.clone(), StoredJob { message: job_msg.clone(), status: JobStatus::Leased, lease_token: Some(lease.clone()) });
                }
                delivery.ack(BasicAckOptions::default()).await.map_err(|e| {
                    QueueError::BackendUnsupported(format!("RabbitMQ ack error: {}", e))
                })?;
                Ok(Some(LeasedJob { job_id, lease_token: lease, job_message: job_msg }))
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
            entry.status = JobStatus::Completed;
            entry.lease_token = None;
            Ok(())
        }

        async fn ack_fail(&self, _ctx: QueueCtx, job_id: JobId, lease_token: LeaseToken, _error: String, _retry_at: Option<DateTime<Utc>>) -> QueueResult<()> {
            let mut jobs = self.jobs.lock().await;
            let entry = jobs.get_mut(&job_id).ok_or(QueueError::JobNotFound(job_id.clone()))?;
            if entry.lease_token.as_ref() != Some(&lease_token) {
                return Err(QueueError::InvalidLeaseToken { job_id });
            }
            entry.status = JobStatus::Failed;
            entry.lease_token = None;
            Ok(())
        }

        async fn cancel(&self, _ctx: QueueCtx, job_id: JobId) -> QueueResult<bool> {
            let mut jobs = self.jobs.lock().await;
            if let Some(entry) = jobs.get_mut(&job_id) {
                entry.status = JobStatus::Canceled;
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

        async fn get_record(&self, _ctx: QueueCtx, job_id: JobId) -> QueueResult<JobRecord> {
            let jobs = self.jobs.lock().await;
            let entry = jobs.get(&job_id).ok_or(QueueError::JobNotFound(job_id.clone()))?;
            Ok(JobRecord {
                job_id,
                job_type: entry.message.job_type.clone(),
                payload: entry.message.payload.clone(),
                status: entry.status.clone(),
                lease_token: entry.lease_token.clone(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
        }

        fn event_stream(&self, _ctx: QueueCtx) -> Pin<Box<dyn Stream<Item = JobEvent> + Send>> {
            Box::pin(futures_util::stream::empty())
        }

        fn capabilities(&self) -> QueueCapabilities {
            QueueCapabilities::default()
        }
    }
}
