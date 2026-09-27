//! Mutable state always uses leader reads. Direct reads are limited to immutable
//! payloads whose unique keys are never reused, with an authoritative fallback.
use super::nats::NatsStore;
use crate::{QueueError, QueueResult};
use async_nats::jetstream::{kv, stream::LastRawMessageErrorKind};
fn error(e: impl std::fmt::Display) -> QueueError {
    QueueError::Internal(e.to_string())
}
impl NatsStore {
    pub(super) async fn leader_entry(
        &self,
        key: impl Into<String>,
    ) -> QueueResult<Option<kv::Entry>> {
        let key = key.into();
        let subject = format!("{}{key}", self.bucket.prefix);
        let message = match self
            .bucket
            .stream
            .get_last_raw_message_by_subject(&subject)
            .await
        {
            Ok(message) => message,
            Err(err) if err.kind() == LastRawMessageErrorKind::NoMessageFound => return Ok(None),
            Err(err) => return Err(error(err)),
        };
        if message.subject.as_str() != subject || message.sequence == 0 {
            return Err(error("invalid authoritative JetStream response"));
        }
        let operation = match message.headers.get("KV-Operation").map(|h| h.as_str()) {
            None | Some("PUT") => kv::Operation::Put,
            Some("DEL") => kv::Operation::Delete,
            Some("PURGE") => kv::Operation::Purge,
            Some(_) => return Err(error("invalid JetStream KV operation")),
        };
        Ok(Some(kv::Entry {
            bucket: self.bucket.name.clone(),
            key,
            value: message.payload,
            revision: message.sequence,
            created: message.time,
            operation,
            delta: 0,
            seen_current: false,
        }))
    }
    pub(super) async fn immutable_payload(&self, key: &str) -> QueueResult<Vec<u8>> {
        if !key.starts_with("p.") {
            return Err(error("direct reads require an immutable payload key"));
        }
        if self.bucket.stream.cached_info().config.allow_direct {
            let subject = format!("{}{key}", self.bucket.prefix);
            // A silent or partitioned direct responder must not consume the
            // whole queue-operation deadline before authoritative fallback.
            if let Ok(Ok(message)) = tokio::time::timeout(
                std::time::Duration::from_millis(250),
                self.bucket.stream.direct_get_last_for_subject(&subject),
            )
            .await
            {
                if message.subject.as_str() == subject
                    && message.sequence > 0
                    && message.headers.get("KV-Operation").is_none()
                {
                    return Ok(message.payload.to_vec());
                }
            }
            // Followers can lag the acknowledged enqueue. Their missing value or
            // tombstone must not establish that an owned job has lost its bytes.
        }
        self.leader_entry(key)
            .await?
            .filter(|entry| entry.operation == kv::Operation::Put)
            .map(|entry| entry.value.to_vec())
            .ok_or_else(|| error("JetStream job payload missing; storage was lost"))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::nats::{NatsBackend, NatsConfig},
        JobMessage, QueueBackend, QueueCtx,
    };
    use futures::StreamExt;
    #[tokio::test]
    #[ignore = "requires disposable JetStream"]
    async fn only_immutable_payloads_use_direct_reads_and_missing_service_falls_back() {
        let url = std::env::var("DOGRS_NATS_URL").unwrap();
        let name = format!("direct_{}", uuid::Uuid::new_v4().simple());
        let backend = NatsBackend::new(NatsConfig {
            url: url.clone(),
            subject: name.clone(),
        })
        .await
        .unwrap();
        assert!(
            backend
                .store
                .bucket
                .stream
                .cached_info()
                .config
                .allow_direct
        );
        let client = async_nats::connect(&url).await.unwrap();
        let mut requests = client
            .subscribe(format!("$JS.API.DIRECT.GET.KV_{name}.>"))
            .await
            .unwrap();
        client.flush().await.unwrap();
        let ctx = QueueCtx::new("direct-owner");
        let id = backend
            .enqueue(
                ctx.clone(),
                JobMessage::new("work", vec![7; 65536], "bytes", "q"),
            )
            .await
            .unwrap();
        assert_eq!(
            backend
                .get_record(ctx.clone(), id.clone())
                .await
                .unwrap()
                .message
                .payload_bytes,
            vec![7; 65536]
        );
        let job = backend.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
        assert_eq!(job.record.message.payload_bytes, vec![7; 65536]);
        backend
            .heartbeat_extend(
                ctx.clone(),
                id.clone(),
                job.lease_token.clone(),
                std::time::Duration::from_secs(10),
            )
            .await
            .unwrap();
        backend
            .ack_complete(ctx.clone(), id, job.lease_token, None)
            .await
            .unwrap();
        let mut count = 0;
        while let Ok(Some(request)) =
            tokio::time::timeout(std::time::Duration::from_millis(30), requests.next()).await
        {
            assert!(
                request.subject.as_str().contains(".p."),
                "mutable state used a replica read: {}",
                request.subject
            );
            count += 1;
        }
        assert!(count >= 2, "immutable bytes should use the direct API");
        assert!(backend
            .store
            .immutable_payload("a.forbidden")
            .await
            .is_err());
        let id = backend
            .enqueue(
                ctx.clone(),
                JobMessage::new("work", vec![9; 65536], "bytes", "q"),
            )
            .await
            .unwrap();
        let js = async_nats::jetstream::new(client);
        let mut config = backend.store.bucket.stream.cached_info().config.clone();
        config.allow_direct = false;
        js.update_stream(config).await.unwrap();
        // The existing client still believes direct service is enabled. Its
        // failed replica lookup must fall back to the authoritative payload.
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                backend.get_record(ctx.clone(), id.clone())
            )
            .await
            .unwrap()
            .unwrap()
            .message
            .payload_bytes,
            vec![9; 65536]
        );
        backend.cancel(ctx.clone(), id).await.unwrap();
        assert_eq!(
            backend
                .purge_terminal_before(ctx, chrono::Utc::now() + chrono::Duration::seconds(1))
                .await
                .unwrap(),
            2
        );
        js.delete_key_value(name).await.unwrap();
    }
}
