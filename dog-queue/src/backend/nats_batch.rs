//! Bounded, conditional atomic publishes (NATS ADR-50). A response is released
//! only after the final durable acknowledgement. Unknown outcomes are never replayed.
use crate::{QueueError, QueueResult};
use async_nats::jetstream::{self, kv, publish::PublishAck, response::Response};
use futures::StreamExt;
use std::{collections::HashSet, time::Duration};
use tokio::sync::{mpsc, oneshot};

const MAX_MESSAGES: usize = 32;
const MAX_BYTES: usize = 2 * 1024 * 1024;
struct Write {
    key: String,
    value: Vec<u8>,
    revision: u64,
}
struct Group {
    queued: Option<std::time::Instant>,
    writes: Vec<Write>,
    reply: oneshot::Sender<QueueResult<Option<Vec<u64>>>>,
}
pub(super) struct BatchWriter {
    updates: mpsc::Sender<Group>,
    enqueues: mpsc::Sender<Group>,
}
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
        // Reserve one execution lane for lease/completion metadata. A large
        // producer backlog must not occupy both durable-write slots or put a
        // lease update behind payload staging in the same atomic batch.
        Self {
            updates: Self::lane(context.clone(), bucket.clone(), false),
            enqueues: Self::lane(context, bucket, true),
        }
    }
    fn lane(context: jetstream::Context, bucket: kv::Store, enqueue: bool) -> mpsc::Sender<Group> {
        let (sender, mut receiver) = mpsc::channel::<Group>(128);
        tokio::spawn(async move {
            let mut deferred = None;
            loop {
                let first = match deferred.take() {
                    Some(group) => group,
                    None => match receiver.recv().await {
                        Some(group) => group,
                        None => break,
                    },
                };
                tokio::task::yield_now().await;
                let mut bytes: usize = first.writes.iter().map(|w| w.value.len()).sum();
                let mut count = first.writes.len();
                let mut keys: HashSet<_> = first.writes.iter().map(|w| w.key.clone()).collect();
                let mut groups = vec![first];
                while count < MAX_MESSAGES {
                    let Ok(group) = receiver.try_recv() else {
                        break;
                    };
                    let size: usize = group.writes.iter().map(|w| w.value.len()).sum();
                    // A logical operation (including an enqueue's payload and metadata)
                    // is indivisible. Distinct expected subjects are required by ADR-50.
                    if count + group.writes.len() > MAX_MESSAGES
                        || bytes + size > MAX_BYTES
                        || group.writes.iter().any(|w| keys.contains(&w.key))
                    {
                        deferred = Some(group);
                        break;
                    }
                    bytes += size;
                    count += group.writes.len();
                    keys.extend(group.writes.iter().map(|w| w.key.clone()));
                    groups.push(group);
                }
                groups.retain(|group| !group.reply.is_closed());
                if groups.is_empty() {
                    continue;
                }
                for group in &mut groups {
                    crate::diagnostics::elapsed(
                        if enqueue {
                            crate::diagnostics::NATS_ENQUEUE_BATCH_QUEUE
                        } else {
                            crate::diagnostics::NATS_UPDATE_BATCH_QUEUE
                        },
                        group.queued.take(),
                    );
                }
                let _execution = crate::diagnostics::Scope::new(if enqueue {
                    crate::diagnostics::NATS_ENQUEUE_BATCH_EXECUTE
                } else {
                    crate::diagnostics::NATS_UPDATE_BATCH_EXECUTE
                });
                let outcomes = tokio::time::timeout(
                    Duration::from_secs(5),
                    execute(&context, &bucket, &groups),
                )
                .await
                .unwrap_or_else(|_| Err(error("timed out; commit outcome may be unknown")));
                match outcomes {
                    Ok(outcomes) => {
                        for (group, outcome) in groups.into_iter().zip(outcomes) {
                            let _ = group.reply.send(outcome);
                        }
                    }
                    Err(e) => {
                        for group in groups {
                            let _ = group.reply.send(Err(e.clone()));
                        }
                    }
                }
            }
        });
        sender
    }
    async fn send(&self, writes: Vec<Write>) -> QueueResult<Option<Vec<u64>>> {
        if writes.is_empty()
            || writes.len() > MAX_MESSAGES
            || writes.iter().map(|w| w.value.len()).sum::<usize>() > MAX_BYTES
        {
            return Err(QueueError::InvalidConfig(
                "JetStream atomic operation exceeds batch bounds".into(),
            ));
        }

        let (reply, receiver) = oneshot::channel();
        let lane = if writes.len() == 1 {
            &self.updates
        } else {
            &self.enqueues
        };
        lane.send(Group {
            writes,
            reply,
            queued: crate::diagnostics::start(),
        })
        .await
        .map_err(|_| error("writer closed"))?;
        receiver
            .await
            .map_err(|_| error("writer interrupted; commit outcome may be unknown"))?
    }
    pub(super) async fn submit(
        &self,
        key: &str,
        value: Vec<u8>,
        revision: u64,
    ) -> QueueResult<Option<u64>> {
        Ok(self
            .send(vec![Write {
                key: key.into(),
                value,
                revision,
            }])
            .await?
            .map(|r| r[0]))
    }
    pub(super) async fn enqueue(
        &self,
        payload_key: String,
        payload: Vec<u8>,
        key: &str,
        metadata: Vec<u8>,
        revision: u64,
    ) -> QueueResult<Option<u64>> {
        Ok(self
            .send(vec![
                Write {
                    key: payload_key,
                    value: payload,
                    revision: 0,
                },
                Write {
                    key: key.into(),
                    value: metadata,
                    revision,
                },
            ])
            .await?
            .map(|r| r[1]))
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
async fn commit(
    context: &jetstream::Context,
    bucket: &kv::Store,
    writes: &[&Write],
) -> QueueResult<Option<Vec<u64>>> {
    if writes.len() == 1 {
        return Ok(single(bucket, writes[0])
            .await?
            .map(|revision| vec![revision]));
    }
    crate::diagnostics::measure(
        crate::diagnostics::NATS_ATOMIC_COMMIT,
        atomic(context, bucket, writes),
    )
    .await
}
async fn execute(
    context: &jetstream::Context,
    bucket: &kv::Store,
    groups: &[Group],
) -> QueueResult<Vec<QueueResult<Option<Vec<u64>>>>> {
    let writes: Vec<_> = groups.iter().flat_map(|g| &g.writes).collect();
    match commit(context, bucket, &writes).await? {
        Some(revisions) => {
            let mut offset = 0;
            Ok(groups
                .iter()
                .map(|g| {
                    let end = offset + g.writes.len();
                    let r = revisions[offset..end].to_vec();
                    offset = end;
                    Ok(Some(r))
                })
                .collect())
        }
        None if groups.len() == 1 => Ok(vec![Ok(None)]),
        // Only a known atomic rejection can reach this branch. Each logical
        // operation remains atomic; an enqueue pair is never split into writes.
        None => {
            // Keep the lane's one-commit bound even during conflicts. Expanding
            // one rejected batch into many concurrent requests defeats admission
            // backpressure precisely when writers contend for the same scopes.
            let mut outcomes = Vec::with_capacity(groups.len());
            for group in groups {
                let writes: Vec<_> = group.writes.iter().collect();
                outcomes.push(commit(context, bucket, &writes).await);
            }
            Ok(outcomes)
        }
    }
}
async fn atomic(
    context: &jetstream::Context,
    bucket: &kv::Store,
    writes: &[&Write],
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
        // Confirm batch start, then pipeline its remaining frames in order.
        // Awaiting every staging reply adds a network round trip per message.
        if i != 0 && i + 1 != writes.len() {
            continue;
        }
        loop {
            let response = replies
                .next()
                .await
                .ok_or_else(|| error("acknowledgement stream closed; outcome may be unknown"))?;
            if response.status.is_some_and(|status| !status.is_success()) {
                return Err(error("server rejected batch request"));
            }
            if response.payload.is_empty() {
                if i == 0 {
                    break;
                }
                // Intermediate staging replies may precede the final durable ack.
                continue;
            }
            match serde_json::from_slice::<Response<PublishAck>>(&response.payload)
                .map_err(error)?
            {
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
        }
    }
    #[tokio::test]
    #[ignore = "requires disposable NATS 2.12+ with atomic publishing"]
    async fn atomic_revisions_and_rejected_batch_preserve_every_key() {
        let (js, bucket) = fixture().await;
        let writes = [
            write("first", 0, vec![1; 65536]),
            write("second", 0, vec![2; 65536]),
        ];
        let revisions = atomic(&js, &bucket, &writes.iter().collect::<Vec<_>>())
            .await
            .unwrap()
            .unwrap();
        for (write, revision) in writes.iter().zip(revisions.iter()) {
            let entry = bucket.entry(&write.key).await.unwrap().unwrap();
            assert_eq!(entry.revision, *revision);
            assert_eq!(&entry.value[..], &write.value);
        }
        let rejected = vec![write("new", 0, vec![3]), write("first", 0, vec![4])];
        assert!(atomic(&js, &bucket, &rejected.iter().collect::<Vec<_>>())
            .await
            .unwrap()
            .is_none());
        assert!(
            bucket.get("new").await.unwrap().is_none(),
            "a rejected batch must not partially commit"
        );
        assert_eq!(
            bucket.entry("first").await.unwrap().unwrap().revision,
            revisions[0]
        );
        let groups: Vec<_> = rejected
            .into_iter()
            .map(|w| Group {
                queued: None,
                writes: vec![w],
                reply: oneshot::channel().0,
            })
            .collect();
        let outcomes = execute(&js, &bucket, &groups).await.unwrap();
        assert!(outcomes[0].as_ref().unwrap().is_some());
        assert!(outcomes[1].as_ref().unwrap().is_none());
        assert_eq!(bucket.get("new").await.unwrap().unwrap().as_ref(), &[3]);
        // A conflicting enqueue pair must stay indivisible even when another
        // independent operation is retried after the mixed batch is rejected.
        let groups = vec![
            Group {
                queued: None,
                writes: vec![write("orphan", 0, vec![7]), write("first", 0, vec![8])],
                reply: oneshot::channel().0,
            },
            Group {
                queued: None,
                writes: vec![write("unrelated", 0, vec![9])],
                reply: oneshot::channel().0,
            },
        ];
        let outcomes = execute(&js, &bucket, &groups).await.unwrap();
        assert!(outcomes[0].as_ref().unwrap().is_none());
        assert!(outcomes[1].as_ref().unwrap().is_some());
        assert!(bucket.get("orphan").await.unwrap().is_none());
        js.delete_key_value(&bucket.name).await.unwrap();
    }
    #[tokio::test]
    #[ignore = "requires disposable NATS 2.12+ with atomic publishing"]
    async fn saturated_admission_cannot_block_metadata_progress() {
        let (js, bucket) = fixture().await;
        let revision = bucket.create("owned", vec![1].into()).await.unwrap();
        let writer = Arc::new(BatchWriter::start(js.clone(), bucket.clone()));
        // Hold every admission slot without publishing. This models an enqueue
        // backlog independently of network speed and makes the regression
        // deterministic: metadata must still reach the live server.
        let mut permits = Vec::new();
        for _ in 0..128 {
            permits.push(writer.enqueues.reserve().await.unwrap());
        }
        let pending = {
            let writer = writer.clone();
            tokio::spawn(async move {
                writer
                    .enqueue("payload".into(), vec![7; 65536], "job", vec![8], 0)
                    .await
            })
        };
        let next = tokio::time::timeout(
            Duration::from_secs(3),
            writer.submit("owned", vec![2], revision),
        )
        .await
        .expect("metadata was blocked by saturated admission")
        .unwrap()
        .unwrap();
        assert!(next > revision);
        assert!(!pending.is_finished());
        assert_eq!(bucket.get("owned").await.unwrap().unwrap().as_ref(), &[2]);
        drop(permits);
        tokio::time::timeout(Duration::from_secs(5), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(bucket.get("payload").await.unwrap().unwrap().len(), 65536);
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
        let mut keys = bucket.keys().await.unwrap();
        let mut payloads = 0;
        while let Some(key) = keys.next().await {
            if key.unwrap().starts_with("p.") {
                payloads += 1;
            }
        }
        assert_eq!(
            payloads, 64,
            "losing enqueue pairs must leave no orphan payloads"
        );
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
