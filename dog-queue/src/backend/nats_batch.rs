//! Bounded, conditional atomic publishes (NATS ADR-50). A response is released
//! only after the final durable acknowledgement. Unknown outcomes are never replayed.
use crate::{QueueError, QueueResult};
use async_nats::jetstream::{self, kv, publish::PublishAck, response::Response};
use futures::{FutureExt, StreamExt, TryStreamExt};
use std::{collections::HashSet, time::Duration};
use tokio::sync::{mpsc, oneshot};

const MAX_MESSAGES: usize = 32;
const MAX_BYTES: usize = 1024 * 1024;
struct Write {
    key: String,
    value: Vec<u8>,
    revision: u64,
    reply: oneshot::Sender<QueueResult<Option<u64>>>,
}
pub(super) struct BatchWriter(mpsc::Sender<Write>);
fn error(e: impl std::fmt::Display) -> QueueError {
    QueueError::Internal(format!("JetStream atomic publish: {e}"))
}
fn conflict(e: &jetstream::Error) -> bool {
    matches!(
        e.error_code(),
        jetstream::ErrorCode::STREAM_WRONG_LAST_SEQUENCE
            | jetstream::ErrorCode::STREAM_WRONG_LAST_SEQUENCE_CONSTANT
    )
}
impl BatchWriter {
    pub(super) fn start(context: jetstream::Context, bucket: kv::Store) -> Self {
        let (sender, mut receiver) = mpsc::channel::<Write>(256);
        tokio::spawn(async move {
            let mut running = tokio::task::JoinSet::new();
            let mut deferred = None;
            loop {
                while running.len() >= 2 {
                    let _ = running.join_next().await;
                }
                let first = match deferred.take() {
                    Some(write) => write,
                    None => match receiver.recv().await {
                        Some(write) => write,
                        None => break,
                    },
                };
                tokio::task::yield_now().await;
                let mut bytes = first.value.len();
                let mut keys = HashSet::from([first.key.clone()]);
                let mut writes = vec![first];
                while writes.len() < MAX_MESSAGES {
                    let Ok(write) = receiver.try_recv() else {
                        break;
                    };
                    // Conditional checks are against the state before the batch.
                    // Never place two writes to the same subject in one batch.
                    if bytes + write.value.len() > MAX_BYTES || !keys.insert(write.key.clone()) {
                        deferred = Some(write);
                        break;
                    }
                    bytes += write.value.len();
                    writes.push(write);
                }
                let context = context.clone();
                let bucket = bucket.clone();
                running.spawn(async move {
                    writes.retain(|write| !write.reply.is_closed());
                    if writes.is_empty() {
                        return;
                    }
                    let result = tokio::time::timeout(
                        Duration::from_secs(5),
                        execute(&context, &bucket, &writes),
                    )
                    .await
                    .unwrap_or_else(|_| Err(error("timed out; commit outcome may be unknown")));
                    match result {
                        Ok(revisions) => {
                            for (write, revision) in writes.into_iter().zip(revisions) {
                                let _ = write.reply.send(Ok(revision));
                            }
                        }
                        Err(e) => {
                            for write in writes {
                                let _ = write.reply.send(Err(e.clone()));
                            }
                        }
                    }
                });
                while running.try_join_next().is_some() {}
            }
            while running.join_next().await.is_some() {}
        });
        Self(sender)
    }
    pub(super) async fn submit(
        &self,
        key: &str,
        value: Vec<u8>,
        revision: u64,
    ) -> QueueResult<Option<u64>> {
        let (reply, receiver) = oneshot::channel();
        self.0
            .send(Write {
                key: key.into(),
                value,
                revision,
                reply,
            })
            .await
            .map_err(|_| error("writer closed"))?;
        receiver
            .await
            .map_err(|_| error("writer interrupted; commit outcome may be unknown"))?
    }
}
async fn single(bucket: &kv::Store, write: &Write) -> QueueResult<Option<u64>> {
    match bucket
        .update(&write.key, write.value.clone().into(), write.revision)
        .await
    {
        Ok(revision) => Ok(Some(revision)),
        Err(e)
            if super::nats_records::revision_conflict(&e)
                || e.kind() == kv::UpdateErrorKind::WrongLastRevision =>
        {
            Ok(None)
        }
        Err(e) => Err(error(e)),
    }
}
async fn execute(
    context: &jetstream::Context,
    bucket: &kv::Store,
    writes: &[Write],
) -> QueueResult<Vec<Option<u64>>> {
    if writes.len() == 1 {
        return Ok(vec![single(bucket, &writes[0]).await?]);
    }
    match crate::diagnostics::measure(
        crate::diagnostics::NATS_ATOMIC_COMMIT,
        atomic(context, bucket, writes),
    )
    .await?
    {
        Some(revisions) => Ok(revisions.into_iter().map(Some).collect()),
        // The server rejected the ENTIRE batch before committing any message.
        // Isolate conflicting keys without discarding unrelated valid updates.
        // No transport, timeout, or malformed-ack failure reaches this branch.
        None => {
            let pending: Vec<_> = writes
                .iter()
                .map(|write| single(bucket, write).boxed())
                .collect();
            futures::stream::iter(pending)
                .buffered(16)
                .try_collect()
                .await
        }
    }
}
async fn atomic(
    context: &jetstream::Context,
    bucket: &kv::Store,
    writes: &[Write],
) -> QueueResult<Option<Vec<u64>>> {
    let client = context.client();
    let inbox = client.new_inbox();
    let mut replies = client.subscribe(inbox.clone()).await.map_err(error)?;
    let id = uuid::Uuid::new_v4().to_string();
    for (i, write) in writes.iter().enumerate() {
        let mut headers = async_nats::HeaderMap::new();
        headers.insert("Nats-Batch-Id", id.as_str());
        headers.insert("Nats-Batch-Sequence", (i + 1).to_string().as_str());
        headers.insert(
            "Nats-Expected-Last-Subject-Sequence",
            write.revision.to_string().as_str(),
        );
        headers.insert("Nats-Expected-Stream", bucket.stream_name.as_str());
        headers.insert("Nats-Required-Api-Level", "2");
        if i + 1 == writes.len() {
            headers.insert("Nats-Batch-Commit", "1");
        }
        let subject = format!(
            "{}{}",
            bucket.put_prefix.as_ref().unwrap_or(&bucket.prefix),
            write.key
        );
        if bucket.use_jetstream_prefix {
            context
                .send_request(
                    subject,
                    async_nats::client::Request::new()
                        .inbox(inbox.clone())
                        .headers(headers)
                        .payload(write.value.clone().into()),
                )
                .await
                .map_err(error)?;
        } else {
            client
                .publish_with_reply_and_headers(
                    subject,
                    inbox.clone(),
                    headers,
                    write.value.clone().into(),
                )
                .await
                .map_err(error)?;
        }
        let response = replies
            .next()
            .await
            .ok_or_else(|| error("acknowledgement stream closed; outcome may be unknown"))?;
        if response.status.is_some_and(|status| !status.is_success()) {
            return Err(error("server rejected batch request"));
        }
        if response.payload.is_empty() {
            if i + 1 == writes.len() {
                return Err(error(
                    "missing final commit acknowledgement; outcome may be unknown",
                ));
            }
            continue;
        }
        match serde_json::from_slice::<Response<PublishAck>>(&response.payload).map_err(error)? {
            Response::Err { error: e } if conflict(&e) => return Ok(None),
            Response::Err { error: e } => return Err(error(e)),
            Response::Ok(ack) => {
                if i + 1 != writes.len()
                    || ack.stream != bucket.stream_name
                    || ack.batch_id.as_deref() != Some(&id)
                    || ack.batch_size != Some(writes.len() as u64)
                    || ack.duplicate
                    || ack.sequence < writes.len() as u64
                {
                    return Err(error(
                        "invalid commit acknowledgement; outcome may be unknown",
                    ));
                }
                let first = ack.sequence - writes.len() as u64 + 1;
                return Ok(Some((first..=ack.sequence).collect()));
            }
        }
    }
    Err(error(
        "batch ended without commit acknowledgement; outcome may be unknown",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        backend::{nats::NatsBackend, QueueBackend},
        JobMessage, QueueCtx,
    };
    use std::sync::Arc;
    async fn fixture() -> (jetstream::Context, kv::Store) {
        let client = async_nats::connect(std::env::var("DOGRS_NATS_URL").unwrap())
            .await
            .unwrap();
        let js = jetstream::new(client);
        let name = format!("batch_{}", uuid::Uuid::new_v4().simple());
        let bucket = js
            .create_key_value(kv::Config {
                bucket: name.clone(),
                storage: jetstream::stream::StorageType::File,
                history: 1,
                ..Default::default()
            })
            .await
            .unwrap();
        let mut config = bucket.stream.cached_info().config.clone();
        config.allow_direct = false;
        config.allow_atomic_publish = true;
        js.update_stream(config).await.unwrap();
        let bucket = js.get_key_value(name).await.unwrap();
        (js, bucket)
    }
    fn write(key: &str, revision: u64, payload: Vec<u8>) -> Write {
        Write {
            key: key.into(),
            revision,
            value: payload,
            reply: oneshot::channel().0,
        }
    }
    #[tokio::test]
    #[ignore = "requires disposable NATS 2.12+ with atomic publishing"]
    async fn atomic_revisions_and_rejected_batch_preserve_every_key() {
        let (js, bucket) = fixture().await;
        let writes = vec![
            write("first", 0, vec![1; 65536]),
            write("second", 0, vec![2; 65536]),
        ];
        let revisions = atomic(&js, &bucket, &writes).await.unwrap().unwrap();
        for (write, revision) in writes.iter().zip(revisions.iter()) {
            let entry = bucket.entry(&write.key).await.unwrap().unwrap();
            assert_eq!(entry.revision, *revision);
            assert_eq!(&entry.value[..], &write.value);
        }
        let rejected = vec![write("new", 0, vec![3]), write("first", 0, vec![4])];
        assert!(atomic(&js, &bucket, &rejected).await.unwrap().is_none());
        assert!(
            bucket.get("new").await.unwrap().is_none(),
            "a rejected batch must not partially commit"
        );
        assert_eq!(
            bucket.entry("first").await.unwrap().unwrap().revision,
            revisions[0]
        );
        let outcomes = execute(&js, &bucket, &rejected).await.unwrap();
        assert!(outcomes[0].is_some());
        assert!(outcomes[1].is_none());
        assert_eq!(bucket.get("new").await.unwrap().unwrap().as_ref(), &[3]);
        js.delete_key_value(&bucket.name).await.unwrap();
    }
    #[tokio::test]
    #[ignore = "requires disposable NATS 2.12+ with atomic publishing"]
    async fn batched_backend_concurrency_dedupe_and_legacy_reopen() {
        let (js, bucket) = fixture().await;
        let backend = Arc::new(
            NatsBackend::from_context(js.clone(), &bucket.name, 1024 * 1024)
                .await
                .unwrap(),
        );
        let mut tasks = tokio::task::JoinSet::new();
        for n in 0..64u8 {
            let backend = backend.clone();
            tasks.spawn(async move {
                backend
                    .enqueue(
                        QueueCtx::new("t"),
                        JobMessage::new("work", vec![n; 65536], "bytes", "q")
                            .with_idempotency_key(n.to_string()),
                    )
                    .await
                    .unwrap()
            });
        }
        let mut ids = HashSet::new();
        while let Some(id) = tasks.join_next().await {
            assert!(ids.insert(id.unwrap()));
        }
        // Concurrent submissions of an active idempotency key return the same job.
        for _ in 0..16 {
            let backend = backend.clone();
            tasks.spawn(async move {
                backend
                    .enqueue(
                        QueueCtx::new("t"),
                        JobMessage::new("work", vec![0; 65536], "bytes", "q")
                            .with_idempotency_key("0"),
                    )
                    .await
                    .unwrap()
            });
        }
        while let Some(id) = tasks.join_next().await {
            assert!(ids.contains(&id.unwrap()));
        }
        let mut consumers = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let backend = backend.clone();
            consumers.spawn(async move {
                let mut completed = Vec::new();
                while let Some(job) = backend.dequeue(QueueCtx::new("t"), &["q"]).await.unwrap() {
                    let bytes = &job.record.message.payload_bytes;
                    assert_eq!(bytes.len(), 65536);
                    assert!(bytes.iter().all(|b| *b == bytes[0]));
                    let id = job.record.job_id;
                    backend
                        .ack_complete(QueueCtx::new("t"), id.clone(), job.lease_token, None)
                        .await
                        .unwrap();
                    completed.push(id);
                }
                completed
            });
        }
        let mut completed = HashSet::new();
        while let Some(result) = consumers.join_next().await {
            for id in result.unwrap() {
                assert!(completed.insert(id));
            }
        }
        assert_eq!(ids, completed);
        let legacy =
            NatsBackend::from_store(js.get_key_value(&bucket.name).await.unwrap()).unwrap();
        for id in ids {
            assert!(legacy
                .get_snapshot(QueueCtx::new("t"), id)
                .await
                .unwrap()
                .status
                .is_terminal());
        }
        js.delete_key_value(&bucket.name).await.unwrap();
    }
}
