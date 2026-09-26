//! RabbitMQ wakeups with a durable job ledger. The supplied channel lets callers
//! configure authenticated TLS connections and their connection recovery policy.
use super::broker::{BrokerBackend, JobLedger, Notifications};
use crate::{QueueError, QueueResult};
use async_trait::async_trait;
use lapin::{options::*, types::FieldTable, BasicProperties, Channel};
use std::sync::Arc;
pub struct RabbitNotifications {
    channel: Channel,
    queue: String,
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
            RabbitNotifications { channel, queue },
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
    async fn receive(&self) -> QueueResult<bool> {
        if let Some(message) = self
            .channel
            .basic_get(&self.queue, BasicGetOptions { no_ack: false })
            .await
            .map_err(error)?
        {
            message
                .delivery
                .ack(BasicAckOptions::default())
                .await
                .map_err(error)?;
            return Ok(true);
        }
        Ok(false)
    }
}
#[allow(clippy::module_inception)] // Preserve the pre-0.2 import path.
pub mod rabbitmq {
    pub use super::{RabbitMqBackend, RabbitNotifications};
}
