//! Kafka notifications plus a durable job ledger. Offsets acknowledge only wakeups;
//! customer jobs, retries and completion are committed in the ledger.
use super::broker::{BrokerBackend, JobLedger, Notifications};
use crate::{QueueError, QueueResult};
use async_trait::async_trait;
use std::sync::Arc;
fn error(e: impl std::fmt::Display) -> QueueError {
    QueueError::Internal(e.to_string())
}

#[cfg(feature = "kafka-rdkafka")]
pub mod rdkafka {
    use super::*;
    use ::rdkafka::{
        consumer::{CommitMode, Consumer, StreamConsumer},
        producer::{FutureProducer, FutureRecord},
    };
    pub struct RdKafkaNotifications {
        producer: FutureProducer,
        consumer: StreamConsumer,
        topic: String,
        subscribed: tokio::sync::Mutex<bool>,
    }
    pub type RdKafkaBackend = BrokerBackend<RdKafkaNotifications>;
    impl RdKafkaBackend {
        /// Configure the supplied clients with TLS/SASL, replication acknowledgements
        /// and a dedicated consumer group. Topic provisioning belongs to the operator.
        pub fn new(
            producer: FutureProducer,
            consumer: StreamConsumer,
            topic: String,
            ledger: Arc<dyn JobLedger>,
        ) -> QueueResult<Self> {
            if topic.is_empty() {
                return Err(QueueError::InvalidConfig("Kafka topic is required".into()));
            }
            consumer.subscribe(&[&topic]).map_err(error)?;
            Ok(Self::with_ledger(
                RdKafkaNotifications {
                    producer,
                    consumer,
                    topic,
                    subscribed: tokio::sync::Mutex::new(true),
                },
                ledger,
            ))
        }
    }
    #[async_trait]
    impl Notifications for RdKafkaNotifications {
        async fn publish(&self) -> QueueResult<()> {
            self.producer
                .send(
                    FutureRecord::<(), str>::to(&self.topic).payload("dogrs-wakeup-v1"),
                    std::time::Duration::from_secs(1),
                )
                .await
                .map_err(|(err, _)| error(err))?;
            Ok(())
        }
        async fn shutdown(&self) {
            let mut subscribed = self.subscribed.lock().await;
            self.consumer.unsubscribe();
            *subscribed = false;
        }
        async fn receive(&self) -> QueueResult<bool> {
            let mut subscribed = self.subscribed.lock().await;
            if !*subscribed {
                self.consumer.subscribe(&[&self.topic]).map_err(error)?;
                *subscribed = true;
            }
            let message = self.consumer.recv().await.map_err(error)?;
            self.consumer
                .commit_message(&message, CommitMode::Async)
                .map_err(error)?;
            Ok(true)
        }
    }
}
#[cfg(feature = "kafka-rdkafka")]
pub use rdkafka::RdKafkaBackend;

#[cfg(feature = "kafka-rskafka")]
pub mod rskafka {
    use super::*;
    use ::rskafka::{
        client::partition::{Compression, OffsetAt, PartitionClient},
        record::Record,
    };
    pub struct RsKafkaNotifications {
        partition: PartitionClient,
        offset: tokio::sync::Mutex<i64>,
    }
    pub type RsKafkaBackend = BrokerBackend<RsKafkaNotifications>;
    impl RsKafkaBackend {
        /// Use a caller-configured TLS/SASL partition client. Wakeups are broadcast
        /// through this partition; only the ledger grants exclusive job ownership.
        pub async fn new(
            partition: PartitionClient,
            ledger: Arc<dyn JobLedger>,
        ) -> QueueResult<Self> {
            let offset = partition
                .get_offset(OffsetAt::Latest)
                .await
                .map_err(error)?;
            Ok(Self::with_ledger(
                RsKafkaNotifications {
                    partition,
                    offset: tokio::sync::Mutex::new(offset),
                },
                ledger,
            ))
        }
    }
    #[async_trait]
    impl Notifications for RsKafkaNotifications {
        async fn publish(&self) -> QueueResult<()> {
            self.partition
                .produce(
                    vec![Record {
                        key: None,
                        value: Some(b"dogrs-wakeup-v1".to_vec()),
                        headers: Default::default(),
                        timestamp: chrono::Utc::now(),
                    }],
                    Compression::default(),
                )
                .await
                .map_err(error)?;
            Ok(())
        }
        async fn receive(&self) -> QueueResult<bool> {
            let mut offset = self.offset.lock().await;
            match self.partition.fetch_records(*offset, 1..65536, 20).await {
                Ok((records, _)) => {
                    if let Some(last) = records.last() {
                        *offset = last.offset + 1;
                    }
                    Ok(!records.is_empty())
                }
                Err(err) => {
                    // Retention can remove old wakeups. Resuming at the current end
                    // is safe because every worker also checks persistent job state.
                    if matches!(
                        &err,
                        ::rskafka::client::error::Error::ServerError {
                            protocol_error:
                                ::rskafka::client::error::ProtocolError::OffsetOutOfRange,
                            ..
                        }
                    ) {
                        *offset = self
                            .partition
                            .get_offset(OffsetAt::Latest)
                            .await
                            .map_err(error)?;
                        tracing::warn!(
                            "Kafka wakeup offset expired; resuming at latest while polling ledger"
                        );
                    }
                    Err(error(err))
                }
            }
        }
    }
}
#[cfg(feature = "kafka-rskafka")]
pub use rskafka::RsKafkaBackend;
#[cfg(feature = "kafka-rdkafka")]
pub type KafkaBackend = RdKafkaBackend;
#[cfg(all(feature = "kafka-rskafka", not(feature = "kafka-rdkafka")))]
pub type KafkaBackend = RsKafkaBackend;
