//! Google Pub/Sub wakeups with authoritative durable job state.
use super::broker::{BrokerBackend, JobLedger, Notifications};
use crate::{QueueError, QueueResult};
use async_trait::async_trait;
use google_cloud_pubsub::{
    client::{Publisher, Subscriber},
    model::Message,
    subscriber::{MessageStream, ShutdownBehavior},
};
use std::sync::Arc;
use tokio::sync::Mutex;
pub struct PubSubNotifications {
    publisher: Publisher,
    subscriber: Subscriber,
    subscription: String,
    session: Mutex<Option<MessageStream>>,
}
pub type GcpPubSubBackend = BrokerBackend<PubSubNotifications>;
fn error(e: impl std::fmt::Display) -> QueueError {
    QueueError::Internal(e.to_string())
}
impl GcpPubSubBackend {
    /// Clients retain the caller's ADC/workload-identity, retry and endpoint settings.
    /// The publisher must target the topic bound to this dedicated subscription.
    pub fn new(
        publisher: Publisher,
        subscriber: Subscriber,
        subscription: String,
        ledger: Arc<dyn JobLedger>,
    ) -> QueueResult<Self> {
        if !subscription.starts_with("projects/") || !subscription.contains("/subscriptions/") {
            return Err(QueueError::InvalidConfig(
                "Full Pub/Sub subscription resource name is required".into(),
            ));
        }
        let session = Mutex::new(None);
        Ok(Self::with_ledger(
            PubSubNotifications {
                publisher,
                subscriber,
                subscription,
                session,
            },
            ledger,
        ))
    }
}
#[async_trait]
impl Notifications for PubSubNotifications {
    async fn publish(&self) -> QueueResult<()> {
        self.publisher
            .publish(Message::new().set_data("dogrs-wakeup-v1"))
            .await
            .map_err(error)?;
        Ok(())
    }
    async fn shutdown(&self) {
        let mut session = self.session.lock().await;
        if let Some(stream) = session.take() {
            let token = stream.shutdown_token();
            drop(stream);
            if tokio::time::timeout(std::time::Duration::from_secs(5), token.shutdown())
                .await
                .is_err()
            {
                tracing::warn!(
                    "Pub/Sub shutdown timed out; acknowledgement outcome may be unknown"
                );
            }
        }
    }
    async fn receive(&self) -> QueueResult<bool> {
        let mut session = self.session.lock().await;
        let stream = session.get_or_insert_with(|| {
            self.subscriber
                .subscribe(&self.subscription)
                .set_max_outstanding_messages(32)
                .set_max_outstanding_bytes(65536)
                .set_shutdown_behavior(ShutdownBehavior::NackImmediately)
                .build()
        });
        match stream.next().await {
            Some(Ok((_, handler))) => {
                handler.ack();
                Ok(true)
            }
            result => {
                *session = None;
                match result {
                    Some(Err(err)) => Err(error(err)),
                    _ => Err(error("Pub/Sub notification stream ended")),
                }
            }
        }
    }
}
#[allow(clippy::module_inception)] // Preserve the pre-0.2 import path.
pub mod gcp_pubsub {
    pub use super::{GcpPubSubBackend, PubSubNotifications};
}
