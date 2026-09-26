// src/backend/rabbitmq.rs

/// Flexible RabbitMQ backend supporting multiple client libraries.
///
/// Features:
///   - `rabbitmq-lapin` – uses the `lapin` crate (default, mature async client).
///   - `rabbitmq-amiquip` – uses the `amiquip` crate (pure‑Rust client).
///
/// The backend selects the first enabled client at compile time.

#[cfg(feature = "rabbitmq-lapin")]
mod impls {
    use async_trait::async_trait;
    use crate::{
        backend::QueueBackend,
        types::LeaseToken,
        JobMessage, JobId, JobRecord, JobStatus, LeasedJob, QueueCtx, QueueResult, QueueError,
        JobEvent, QueueCapabilities,
    };
    use chrono::{DateTime, Utc};
    use futures_core::Stream;
    use futures::stream::StreamExt;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use std::collections::HashMap;

    // ---------------------------------------------------------------
    // Common in‑memory job store used by all implementations.
    // ---------------------------------------------------------------
    #[derive(Debug, Clone)]
    struct StoredJob {
        message: JobMessage,
        status: JobStatus,
        lease_token: Option<LeaseToken>,
    }

    // Helper functions to convert JSON values to lapin AMQP values
    #[cfg(feature = "rabbitmq-lapin")]
    fn to_lapin_field_table(map: &HashMap<String, serde_json::Value>) -> lapin::types::FieldTable {
        let mut table = lapin::types::FieldTable::default();
        for (k, v) in map {
            table.insert(k.clone().into(), json_to_lapin_value(v));
        }
        table
    }

    #[cfg(feature = "rabbitmq-lapin")]
    fn json_to_lapin_value(val: &serde_json::Value) -> lapin::types::AMQPValue {
        match val {
            serde_json::Value::Null => lapin::types::AMQPValue::Void,
            serde_json::Value::Bool(b) => lapin::types::AMQPValue::Boolean(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    lapin::types::AMQPValue::LongLongInt(i)
                } else if let Some(f) = n.as_f64() {
                    lapin::types::AMQPValue::Double(f)
                } else {
                    lapin::types::AMQPValue::Void
                }
            }
            serde_json::Value::String(s) => lapin::types::AMQPValue::LongString(s.clone().into()),
            serde_json::Value::Array(arr) => {
                let values: Vec<lapin::types::AMQPValue> = arr.iter().map(json_to_lapin_value).collect();
                lapin::types::AMQPValue::FieldArray(values.into())
            }
            serde_json::Value::Object(obj) => {
                let mut table = lapin::types::FieldTable::default();
                for (k, v) in obj {
                    table.insert(k.clone().into(), json_to_lapin_value(v));
                }
                lapin::types::AMQPValue::FieldTable(table)
            }
        }
    }

    // ---------------------------------------------------------------
    // Lapin implementation (feature = "rabbitmq-lapin").
    // ---------------------------------------------------------------
    #[cfg(feature = "rabbitmq-lapin")]
    mod lapin_impl {
        use super::*;
        use lapin::{
            options::{
                BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions,
            },
            BasicProperties, Channel, Connection, ConnectionProperties, Consumer,
        };

        #[derive(Debug, Clone)]
        pub struct LapinBackend {
            channel: Channel,
            queue_name: String,
            exchange: String,
            routing_key: String,
            jobs: Arc<Mutex<HashMap<JobId, StoredJob>>>,
        }

        impl LapinBackend {
            pub async fn new(config: super::super::RabbitMqConfig) -> Result<Self, QueueError> {
                let conn = Connection::connect(&config.uri, ConnectionProperties::default())
                    .await
                    .map_err(|e| QueueError::BackendUnsupported(format!("RabbitMQ connect error: {}", e)))?;
                let channel = conn.create_channel().await.map_err(|e| {
                    QueueError::BackendUnsupported(format!("RabbitMQ channel error: {}", e))
                })?;

                // Declare exchange if present.
                let exchange_name = config.exchange.clone().unwrap_or_default();
                if !exchange_name.is_empty() {
                    channel
                        .exchange_declare(
                            &exchange_name,
                            lapin::ExchangeKind::Direct,
                            lapin::options::ExchangeDeclareOptions::default(),
                            lapin::types::FieldTable::default(),
                        )
                        .await
                        .map_err(|e| {
                            QueueError::BackendUnsupported(format!("RabbitMQ exchange declare error: {}", e))
                        })?;
                }

                let lapin_args = to_lapin_field_table(&config.arguments);
                channel
                    .queue_declare(
                        &config.queue_name,
                        QueueDeclareOptions { durable: config.durable, ..Default::default() },
                        lapin_args,
                    )
                    .await
                    .map_err(|e| {
                        QueueError::BackendUnsupported(format!("RabbitMQ queue declare error: {}", e))
                    })?;

                if !exchange_name.is_empty() {
                    let routing = config.routing_key.clone().unwrap_or_else(|| config.queue_name.clone());
                    channel
                        .queue_bind(
                            &config.queue_name,
                            &exchange_name,
                            &routing,
                            lapin::options::QueueBindOptions::default(),
                            lapin::types::FieldTable::default(),
                        )
                        .await
                        .map_err(|e| {
                            QueueError::BackendUnsupported(format!("RabbitMQ bind error: {}", e))
                        })?;
                }

                if let Some(pref) = config.prefetch {
                    channel.basic_qos(pref, lapin::options::BasicQosOptions::default()).await.map_err(|e| {
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
                let stored = StoredJob { message: message.clone(), status: JobStatus::Enqueued, lease_token: None };
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
                        payload.as_bytes(),
                        BasicProperties::default().with_content_type("application/json".into()),
                    )
                    .await
                    .map_err(|e| QueueError::BackendUnsupported(format!("RabbitMQ publish error: {}", e)))?;
                Ok(job_id)
            }

            async fn dequeue(&self, _ctx: QueueCtx, _queues: &[&str]) -> QueueResult<Option<LeasedJob>> {
                let mut consumer: Consumer = self.channel
                    .basic_consume(
                        &self.queue_name,
                        "dog_queue_consumer",
                        BasicConsumeOptions::default(),
                        lapin::types::FieldTable::default(),
                    )
                    .await
                    .map_err(|e| QueueError::BackendUnsupported(format!("RabbitMQ consume error: {}", e)))?;

                if let Some(delivery) = consumer.next().await {
                    let delivery = delivery.map_err(|e| QueueError::BackendUnsupported(format!("RabbitMQ delivery error: {}", e)))?;
                    let job_msg: JobMessage = serde_json::from_slice(&delivery.data)
                        .map_err(|e| QueueError::SerializationError(e.to_string()))?;
                    let job_id = JobId::new();
                    let lease = LeaseToken::new();
                    let lease_until = Utc::now() + chrono::Duration::seconds(300);

                    {
                        let mut jobs = self.jobs.lock().await;
                        jobs.insert(
                            job_id.clone(),
                            StoredJob {
                                message: job_msg.clone(),
                                status: JobStatus::Processing { lease_until },
                                lease_token: Some(lease.clone()),
                            },
                        );
                    }
                    delivery.ack(BasicAckOptions::default()).await.map_err(|e| {
                        QueueError::BackendUnsupported(format!("RabbitMQ ack error: {}", e))
                    })?;

                    let record = JobRecord {
                        job_id: job_id.clone(),
                        tenant_id: _ctx.tenant_id.clone(),
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

            async fn ack_fail(&self, _ctx: QueueCtx, job_id: JobId, lease_token: LeaseToken, error: String, _retry_at: Option<DateTime<Utc>>) -> QueueResult<()> {
                let mut jobs = self.jobs.lock().await;
                let entry = jobs.get_mut(&job_id).ok_or(QueueError::JobNotFound(job_id.clone()))?;
                if entry.lease_token.as_ref() != Some(&lease_token) {
                    return Err(QueueError::InvalidLeaseToken { job_id });
                }
                entry.status = JobStatus::Failed { failed_at: Utc::now(), error };
                entry.lease_token = None;
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

            async fn get_record(&self, _ctx: QueueCtx, job_id: JobId) -> QueueResult<JobRecord> {
                let jobs = self.jobs.lock().await;
                let entry = jobs.get(&job_id).ok_or(QueueError::JobNotFound(job_id.clone()))?;
                Ok(JobRecord {
                    job_id,
                    tenant_id: _ctx.tenant_id.clone(),
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

            fn capabilities(&self) -> QueueCapabilities {
                QueueCapabilities::default()
            }
        }
    }
    // Public façade that selects the implementation based on enabled features.
    // ---------------------------------------------------------------
    #[derive(Clone)]
    pub struct RabbitMqBackend {
        inner: Arc<dyn QueueBackend + Send + Sync>,
    }

    impl std::fmt::Debug for RabbitMqBackend {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("RabbitMqBackend").finish_non_exhaustive()
        }
    }

    impl RabbitMqBackend {
        pub async fn new(config: super::RabbitMqConfig) -> Result<Self, QueueError> {
            let impl_ = lapin_impl::LapinBackend::new(config).await?;
            Ok(Self { inner: Arc::new(impl_) })
        }
    }

    #[async_trait]
    impl QueueBackend for RabbitMqBackend {
        async fn enqueue(&self, ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId> {
            self.inner.enqueue(ctx, message).await
        }
        async fn dequeue(&self, ctx: QueueCtx, queues: &[&str]) -> QueueResult<Option<LeasedJob>> {
            self.inner.dequeue(ctx, queues).await
        }
        async fn ack_complete(&self, ctx: QueueCtx, job_id: JobId, lease_token: LeaseToken, result_ref: Option<String>) -> QueueResult<()> {
            self.inner.ack_complete(ctx, job_id, lease_token, result_ref).await
        }
        async fn ack_fail(&self, ctx: QueueCtx, job_id: JobId, lease_token: LeaseToken, error: String, retry_at: Option<DateTime<Utc>>) -> QueueResult<()> {
            self.inner.ack_fail(ctx, job_id, lease_token, error, retry_at).await
        }
        async fn cancel(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<bool> {
            self.inner.cancel(ctx, job_id).await
        }
        async fn get_status(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<JobStatus> {
            self.inner.get_status(ctx, job_id).await
        }
        async fn get_record(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<JobRecord> {
            self.inner.get_record(ctx, job_id).await
        }
        fn event_stream(&self, ctx: QueueCtx) -> Pin<Box<dyn Stream<Item = JobEvent> + Send>> {
            self.inner.event_stream(ctx)
        }
        fn capabilities(&self) -> QueueCapabilities {
            self.inner.capabilities()
        }
    }
}

// Re‑export the façade when any RabbitMQ feature is active.
#[cfg(feature = "rabbitmq-lapin")]
pub use impls::RabbitMqBackend;

// Configuration struct.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq)]
pub struct RabbitMqConfig {
    pub uri: String,
    pub queue_name: String,
    pub exchange: Option<String>,
    pub routing_key: Option<String>,
    pub durable: bool,
    pub prefetch: Option<u16>,
    pub arguments: std::collections::HashMap<String, serde_json::Value>,
    pub prefetch_count: Option<u16>,
    pub prefetch_global: Option<bool>,
    pub auto_ack: Option<bool>,
    pub exclusive: Option<bool>,
    pub no_local: Option<bool>,
    pub no_wait: Option<bool>,
    pub nowait: Option<bool>,
}

// Guard the module with the generic rabbitmq feature for backward compatibility.
#[cfg(feature = "rabbitmq")]
pub mod rabbitmq {
    pub use super::RabbitMqBackend;
    pub use super::RabbitMqConfig;
}
