// src/backend/kafka.rs

/// Flexible Kafka backend supporting multiple client libraries.
///
/// Features:
///   - `kafka-rdkafka` – uses the `rdkafka` crate (default, based on librdkafka).
///   - `kafka-rskafka` – uses the `rskafka` crate (pure‑Rust client).
///
/// The backend selects the first enabled client at compile time.

#[cfg(any(feature = "kafka-rdkafka", feature = "kafka-rskafka"))]
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

    // ---------------------------------------------------------------
    // rdkafka implementation (feature = "kafka-rdkafka").
    // ---------------------------------------------------------------
    #[cfg(feature = "kafka-rdkafka")]
    mod rdkafka_impl {
        use super::*;
        use rdkafka::{
            producer::{FutureProducer, FutureRecord},
            consumer::{StreamConsumer, Consumer},
            message::Message,
            ClientConfig as KafkaConfig,
        };
        use std::time::Duration;

        pub struct RdKafkaBackend {
            producer: FutureProducer,
            consumer: StreamConsumer,
            topic: String,
            jobs: Arc<Mutex<HashMap<JobId, StoredJob>>>,
        }

        impl std::fmt::Debug for RdKafkaBackend {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("RdKafkaBackend")
                    .field("topic", &self.topic)
                    .finish()
            }
        }

        impl RdKafkaBackend {
            pub async fn new(config: super::super::KafkaConfigStruct) -> Result<Self, QueueError> {
                // Producer configuration.
                let mut prod_cfg = KafkaConfig::new();
                prod_cfg.set("bootstrap.servers", &config.brokers);
                let producer: FutureProducer = prod_cfg.create()
                    .map_err(|e| QueueError::BackendUnsupported(format!("Kafka producer error: {}", e)))?;
                
                // Consumer configuration.
                let mut cons_cfg = KafkaConfig::new();
                cons_cfg.set("bootstrap.servers", &config.brokers);
                cons_cfg.set("group.id", &config.group_id);
                cons_cfg.set("enable.auto.commit", "false");
                cons_cfg.set("auto.offset.reset", "earliest");
                let consumer: StreamConsumer = cons_cfg.create()
                    .map_err(|e| QueueError::BackendUnsupported(format!("Kafka consumer error: {}", e)))?;
                consumer.subscribe(&[&config.topic])
                    .map_err(|e| QueueError::BackendUnsupported(format!("Kafka subscribe error: {}", e)))?;

                Ok(Self {
                    producer,
                    consumer,
                    topic: config.topic,
                    jobs: Arc::new(Mutex::new(HashMap::new())),
                })
            }
        }

        #[async_trait]
        impl QueueBackend for RdKafkaBackend {
            async fn enqueue(&self, _ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId> {
                let job_id = JobId::new();
                let stored = StoredJob { message: message.clone(), status: JobStatus::Enqueued, lease_token: None };
                {
                    let mut jobs = self.jobs.lock().await;
                    jobs.insert(job_id.clone(), stored);
                }
                let payload = serde_json::to_string(&message)
                    .map_err(|e| QueueError::SerializationError(e.to_string()))?;
                let record = FutureRecord::to(&self.topic)
                    .payload(payload.as_str())
                    .key(job_id.as_str());
                self.producer.send(record, Duration::from_secs(0)).await
                    .map_err(|(e, _)| QueueError::BackendUnsupported(format!("Kafka enqueue error: {}", e)))?;
                Ok(job_id)
            }

            async fn dequeue(&self, _ctx: QueueCtx, _queues: &[&str]) -> QueueResult<Option<LeasedJob>> {
                let mut stream = self.consumer.stream();
                if let Some(Ok(msg)) = stream.next().await {
                    let payload = msg.payload_view::<str>()
                        .transpose()
                        .map_err(|e| QueueError::BackendUnsupported(format!("UTF-8 decode error: {}", e)))?
                        .ok_or_else(|| QueueError::BackendUnsupported("Kafka payload missing".into()))?;
                    let job_msg: JobMessage = serde_json::from_str(payload)
                        .map_err(|e| QueueError::SerializationError(e.to_string()))?;
                    
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
                        self.consumer.commit_message(&msg, rdkafka::consumer::CommitMode::Async).ok();

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

            async fn ack_fail(&self, _ctx: QueueCtx, job_id: JobId, lease_token: LeaseToken, error: String, retry_at: Option<DateTime<Utc>>) -> QueueResult<()> {
                let mut jobs = self.jobs.lock().await;
                let entry = jobs.get_mut(&job_id).ok_or(QueueError::JobNotFound(job_id.clone()))?;
                if entry.lease_token.as_ref() != Some(&lease_token) {
                    return Err(QueueError::InvalidLeaseToken { job_id });
                }
                entry.status = JobStatus::Failed { failed_at: Utc::now(), error };
                entry.lease_token = None;
                
                if let Some(at) = retry_at {
                    let now = Utc::now();
                    if at > now {
                        let delay = (at - now).to_std().unwrap_or(Duration::from_secs(0));
                        let payload = serde_json::to_string(&entry.message)
                            .map_err(|e| QueueError::SerializationError(e.to_string()))?;
                        let producer = self.producer.clone();
                        let topic = self.topic.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(delay).await;
                            let _ = producer.send(
                                FutureRecord::to(&topic)
                                    .payload(payload.as_str())
                                    .key(job_id.as_str()),
                                Duration::from_secs(0)
                            ).await;
                        });
                    }
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

    // ---------------------------------------------------------------
    // rskafka implementation (feature = "kafka-rskafka").
    // ---------------------------------------------------------------
    #[cfg(all(feature = "kafka-rskafka", not(feature = "kafka-rdkafka")))]
    mod rskafka_impl {
        use super::*;
        use std::sync::atomic::{AtomicI64, Ordering};

        pub struct RsKafkaBackend {
            client: rskafka::client::partition::PartitionClient,
            topic: String,
            next_offset: Arc<AtomicI64>,
            jobs: Arc<Mutex<HashMap<JobId, StoredJob>>>,
        }

        impl std::fmt::Debug for RsKafkaBackend {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("RsKafkaBackend")
                    .field("topic", &self.topic)
                    .finish()
            }
        }

        impl RsKafkaBackend {
            pub async fn new(config: super::super::KafkaConfigStruct) -> Result<Self, QueueError> {
                let client = rskafka::client::ClientBuilder::new(vec![config.brokers])
                    .build()
                    .await
                    .map_err(|e| QueueError::BackendUnsupported(format!("rskafka build error: {}", e)))?;
                
                let partition_client = client
                    .partition_client(config.topic.clone(), 0, rskafka::client::partition::UnknownTopicHandling::Retry)
                    .await
                    .map_err(|e| QueueError::BackendUnsupported(format!("rskafka partition client error: {}", e)))?;

                Ok(Self {
                    client: partition_client,
                    topic: config.topic,
                    next_offset: Arc::new(AtomicI64::new(0)),
                    jobs: Arc::new(Mutex::new(HashMap::new())),
                })
            }
        }

        #[async_trait]
        impl QueueBackend for RsKafkaBackend {
            async fn enqueue(&self, _ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId> {
                let job_id = JobId::new();
                let stored = StoredJob { message: message.clone(), status: JobStatus::Enqueued, lease_token: None };
                {
                    let mut jobs = self.jobs.lock().await;
                    jobs.insert(job_id.clone(), stored);
                }
                let payload = serde_json::to_string(&message)
                    .map_err(|e| QueueError::SerializationError(e.to_string()))?;
                
                let record = rskafka::record::Record {
                    key: Some(job_id.as_str().as_bytes().to_vec()),
                    value: Some(payload.into_bytes()),
                    headers: std::collections::BTreeMap::new(),
                    timestamp: Utc::now(),
                };

                self.client.produce(vec![record], rskafka::client::partition::Compression::default())
                    .await
                    .map_err(|e| QueueError::BackendUnsupported(format!("rskafka produce error: {}", e)))?;
                Ok(job_id)
            }

            async fn dequeue(&self, _ctx: QueueCtx, _queues: &[&str]) -> QueueResult<Option<LeasedJob>> {
                let offset = self.next_offset.load(Ordering::SeqCst);
                match self.client.fetch_records(offset, 1..1_000_000, 100).await {
                    Ok((records, _)) => {
                        if let Some(record) = records.first() {
                            let payload = record.record.value.as_ref().ok_or_else(|| QueueError::BackendUnsupported("rskafka value missing".into()))?;
                            let job_msg: JobMessage = serde_json::from_slice(payload)
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

                            self.next_offset.fetch_add(1, Ordering::SeqCst);

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
                    Err(e) => Err(QueueError::BackendUnsupported(format!("rskafka fetch error: {}", e))),
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

    // ---------------------------------------------------------------
    // Public façade that selects the implementation based on enabled features.
    // ---------------------------------------------------------------
    #[derive(Clone)]
    pub struct KafkaBackend {
        inner: Arc<dyn QueueBackend + Send + Sync>,
    }

    impl std::fmt::Debug for KafkaBackend {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("KafkaBackend").finish_non_exhaustive()
        }
    }

    impl KafkaBackend {
        pub async fn new(config: super::KafkaConfigStruct) -> Result<Self, QueueError> {
            #[cfg(feature = "kafka-rdkafka")]
            {
                let impl_ = rdkafka_impl::RdKafkaBackend::new(config.clone()).await?;
                return Ok(Self { inner: Arc::new(impl_) });
            }
            #[cfg(all(not(feature = "kafka-rdkafka"), feature = "kafka-rskafka"))]
            {
                let impl_ = rskafka_impl::RsKafkaBackend::new(config.clone()).await?;
                return Ok(Self { inner: Arc::new(impl_) });
            }
            #[allow(unreachable_code)]
            Err(QueueError::BackendUnsupported("No Kafka client feature enabled".into()))
        }
    }

    #[async_trait]
    impl QueueBackend for KafkaBackend {
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

// Re‑export the façade when any Kafka feature is active.
#[cfg(any(feature = "kafka-rdkafka", feature = "kafka-rskafka"))]
pub use impls::KafkaBackend;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KafkaConfigStruct {
    pub brokers: String,
    pub topic: String,
    pub group_id: String,
}
