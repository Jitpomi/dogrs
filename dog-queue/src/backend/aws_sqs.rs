//! SQS wakeups with a durable job ledger. Uses a caller-configured SDK client,
//! including credential refresh, role credentials, TLS and endpoint configuration.
use super::broker::{BrokerBackend, JobLedger, Notifications};
use crate::{QueueError, QueueResult};
use async_trait::async_trait;
use aws_sdk_sqs::Client;
use std::sync::Arc;
pub struct SqsNotifications {
    client: Client,
    queue_url: String,
}
pub type AwsSqsBackend = BrokerBackend<SqsNotifications>;
impl AwsSqsBackend {
    pub fn new(client: Client, queue_url: String, ledger: Arc<dyn JobLedger>) -> QueueResult<Self> {
        if queue_url.is_empty() {
            return Err(QueueError::InvalidConfig(
                "SQS queue URL is required".into(),
            ));
        }
        Ok(Self::with_ledger(
            SqsNotifications { client, queue_url },
            ledger,
        ))
    }
}
fn error(e: impl std::fmt::Display) -> QueueError {
    QueueError::Internal(e.to_string())
}
#[async_trait]
impl Notifications for SqsNotifications {
    async fn publish(&self) -> QueueResult<()> {
        let mut request = self
            .client
            .send_message()
            .queue_url(&self.queue_url)
            .message_body("dogrs-wakeup-v1");
        if self.queue_url.ends_with(".fifo") {
            request = request
                .message_group_id(uuid::Uuid::new_v4().to_string())
                .message_deduplication_id(uuid::Uuid::new_v4().to_string());
        }
        request.send().await.map_err(error)?;
        Ok(())
    }
    async fn receive(&self) -> QueueResult<bool> {
        let response = self
            .client
            .receive_message()
            .queue_url(&self.queue_url)
            .max_number_of_messages(10)
            .wait_time_seconds(20)
            .send()
            .await
            .map_err(error)?;
        let received = !response.messages().is_empty();
        let entries: Vec<_> = response
            .messages()
            .iter()
            .enumerate()
            .filter_map(|(index, message)| {
                message.receipt_handle().map(|receipt| {
                    aws_sdk_sqs::types::DeleteMessageBatchRequestEntry::builder()
                        .id(index.to_string())
                        .receipt_handle(receipt)
                        .build()
                        .map_err(error)
                })
            })
            .collect::<QueueResult<_>>()?;
        if !entries.is_empty() {
            let result = self
                .client
                .delete_message_batch()
                .queue_url(&self.queue_url)
                .set_entries(Some(entries))
                .send()
                .await
                .map_err(error)?;
            if !result.failed().is_empty() {
                return Err(error(
                    "SQS failed to acknowledge some wakeups; ledger jobs remain available",
                ));
            }
        }
        Ok(received)
    }
}
#[allow(clippy::module_inception)] // Preserve the pre-0.2 import path.
pub mod aws_sqs {
    pub use super::{AwsSqsBackend, SqsNotifications};
}
