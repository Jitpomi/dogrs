//! RabbitMQ wakeups with a durable job ledger. The supplied channel lets callers
//! configure authenticated TLS connections and their connection recovery policy.
use super::broker::{BrokerBackend, JobLedger, Notifications};
use crate::{QueueError, QueueResult};
use async_trait::async_trait;
use futures::StreamExt;
use lapin::{options::*, types::FieldTable, BasicProperties, Channel};
use std::sync::Arc;
use tokio::sync::Mutex;
pub struct RabbitNotifications {
    channel: Channel,
    queue: String,
    consumer: Mutex<Option<lapin::Consumer>>,
}
pub type RabbitMqBackend = BrokerBackend<RabbitNotifications>;
fn error(e: impl std::fmt::Display) -> QueueError {
    QueueError::Internal(e.to_string())
}
impl RabbitMqBackend {
    /// Uses a dedicated durable queue and confirms every publication. The queue
    /// should be provisioned as a quorum queue when replicated delivery is required.
    pub async fn new(
        channel: Channel,
        queue: String,
        ledger: Arc<dyn JobLedger>,
    ) -> QueueResult<Self> {
        if queue.is_empty() {
            return Err(QueueError::InvalidConfig(
                "RabbitMQ queue name is required".into(),
            ));
        }
        channel
            .queue_declare(
                &queue,
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .map_err(error)?;
        channel
            .confirm_select(ConfirmSelectOptions::default())
            .await
            .map_err(error)?;
        Ok(Self::with_ledger(
            RabbitNotifications {
                channel,
                queue,
                consumer: Mutex::new(None),
            },
            ledger,
        ))
    }
}
#[async_trait]
impl Notifications for RabbitNotifications {
    async fn publish(&self) -> QueueResult<()> {
        let confirmed = self
            .channel
            .basic_publish(
                "",
                &self.queue,
                BasicPublishOptions {
                    mandatory: true,
                    ..Default::default()
                },
                b"dogrs-wakeup-v1",
                BasicProperties::default().with_delivery_mode(2),
            )
            .await
            .map_err(error)?
            .await
            .map_err(error)?;
        match confirmed {
            lapin::publisher_confirm::Confirmation::Ack(None) => Ok(()),
            _ => Err(error("RabbitMQ rejected or returned the notification")),
        }
    }
    async fn shutdown(&self) {
        let mut slot = self.consumer.lock().await;
        let Some(consumer) = slot.as_mut() else {
            return;
        };
        // Cancel first, then settle the bounded prefetch backlog. These are only
        // hints: acknowledging them cannot complete or remove a ledger job.
        let cleanup = async {
            self.channel
                .basic_cancel(consumer.tag().as_str(), BasicCancelOptions::default())
                .await?;
            while let Some(delivery) = consumer.next().await {
                delivery?.ack(BasicAckOptions::default()).await?;
            }
            // A round trip on this channel also orders the preceding ack frames.
            self.channel
                .queue_declare(
                    &self.queue,
                    QueueDeclareOptions {
                        passive: true,
                        ..Default::default()
                    },
                    FieldTable::default(),
                )
                .await?;
            Ok::<_, lapin::Error>(())
        };
        match tokio::time::timeout(std::time::Duration::from_secs(5), cleanup).await {
            Ok(Ok(())) => {},
            result => tracing::warn!(?result, "RabbitMQ shutdown incomplete; close the supplied channel to release outstanding deliveries"),
        }
        *slot = None;
    }
    async fn receive(&self) -> QueueResult<bool> {
        let mut consumer = self.consumer.lock().await;
        if consumer.is_none() {
            self.channel
                .basic_qos(32, BasicQosOptions::default())
                .await
                .map_err(error)?;
            *consumer = Some(
                self.channel
                    .basic_consume(
                        &self.queue,
                        "",
                        BasicConsumeOptions::default(),
                        FieldTable::default(),
                    )
                    .await
                    .map_err(error)?,
            );
        }
        match consumer.as_mut().unwrap().next().await {
            Some(Ok(delivery)) => {
                delivery
                    .ack(BasicAckOptions::default())
                    .await
                    .map_err(error)?;
                return Ok(true);
            }
            Some(Err(err)) => {
                *consumer = None;
                return Err(error(err));
            }
            None => {
                *consumer = None;
            }
        }
        Ok(false)
    }
}
#[allow(clippy::module_inception)] // Preserve the pre-0.2 import path.
pub mod rabbitmq {
    pub use super::{RabbitMqBackend, RabbitNotifications};
}
