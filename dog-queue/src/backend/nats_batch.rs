//! Bounded, conditional atomic publishes (NATS ADR-50). A response is released
//! only after the final durable acknowledgement. Unknown outcomes are never replayed.
use crate::{QueueError, QueueResult};
use async_nats::jetstream::{self, kv, publish::PublishAck, response::Response};
use futures::StreamExt;
use std::{collections::HashSet, time::Duration};
use tokio::sync::{mpsc, oneshot};

const MAX_MESSAGES: usize = 128;
pub(super) const ADMISSION_CAPACITY: usize = 128;
const MAX_BYTES: usize = 2 * 1024 * 1024;
// JetStream closes a Raft append after the entry that crosses 256 KiB,
// rather than before it. Include the logical operation crossing the target;
// enqueues put their small metadata first and payload last so the crossing
// entry can also finish the atomic batch. A larger operation stays intact.
const TARGET_BATCH_BYTES: usize = 256 * 1024;
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
        // producer backlog must not occupy every durable-write slot or put a
        // lease update behind payload staging in the same atomic batch.
        Self {
            updates: Self::lane(context.clone(), bucket.clone(), false),
            enqueues: Self::lane(context, bucket, true),
        }
    }
    fn lane(context: jetstream::Context, bucket: kv::Store, enqueue: bool) -> mpsc::Sender<Group> {
        let (sender, mut receiver) = mpsc::channel::<Group>(ADMISSION_CAPACITY);
        tokio::spawn(async move {
            // Small atomic batches should overlap their acknowledgement waits.
            // One in-flight 3-job batch at 100 ms caps admission at 30 jobs/s
            // regardless of how much unused capacity the provider has. Retain
            // a separate metadata lane so producer pipelining cannot consume it.
            let concurrency = if enqueue {
                std::env::var("DOGRS_NATS_ENQUEUE_CONCURRENCY")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(4)
            } else {
                std::env::var("DOGRS_NATS_UPDATE_CONCURRENCY")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1)
            };
            let mut running = tokio::task::JoinSet::new();
            let mut deferred = None;
            loop {
                while running.len() >= concurrency {
                    let _ = running.join_next().await;
                }
                let first = match deferred.take() {
                    Some(group) => group,
                    None => match receiver.recv().await {
                        Some(group) => group,
                        None => break,
                    },
                };
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
                        || bytes >= TARGET_BATCH_BYTES
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
                if bytes < TARGET_BATCH_BYTES && deferred.is_none() && count < MAX_MESSAGES {
                    let deadline = tokio::time::Instant::now() + Duration::from_millis(2);
                    while count < MAX_MESSAGES {
                        match tokio::time::timeout_at(deadline, receiver.recv()).await {
                            Ok(Some(group)) => {
                                let size: usize = group.writes.iter().map(|w| w.value.len()).sum();
                                if count + group.writes.len() > MAX_MESSAGES
                                    || bytes >= TARGET_BATCH_BYTES
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
                            _ => break,
                        }
                    }
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
                let context = context.clone();
                let bucket = bucket.clone();
                running.spawn(async move {
                    let _execution = crate::diagnostics::Scope::new(if enqueue {
                        crate::diagnostics::NATS_ENQUEUE_BATCH_EXECUTE
                    } else {
                        crate::diagnostics::NATS_UPDATE_BATCH_EXECUTE
                    });
                    let outcomes = execute(&context, &bucket, &groups).await;
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
                });
                while running.try_join_next().is_some() {}
            }
            while running.join_next().await.is_some() {}
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
                // Atomic visibility makes this order safe: metadata is not
                // discoverable until its payload commits in the same batch.
                Write {
                    key: key.into(),
                    value: metadata,
                    revision,
                },
                Write {
                    key: payload_key,
                    value: payload,
                    revision: 0,
                },
            ])
            .await?
            .map(|r| r[0]))
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
    execute_groups(groups, |writes| async move {
        commit(context, bucket, &writes).await
    })
    .await
}
async fn execute_groups<'a, F, Fut>(
    groups: &'a [Group],
    attempt: F,
) -> QueueResult<Vec<QueueResult<Option<Vec<u64>>>>>
where
    F: FnMut(Vec<&'a Write>) -> Fut,
    Fut: std::future::Future<Output = QueueResult<Option<Vec<u64>>>>,
{
    execute_groups_until(
        groups,
        tokio::time::Instant::now() + Duration::from_secs(5),
        attempt,
    )
    .await
}
async fn execute_groups_until<'a, F, Fut>(
    groups: &'a [Group],
    deadline: tokio::time::Instant,
    mut attempt: F,
) -> QueueResult<Vec<QueueResult<Option<Vec<u64>>>>>
where
    F: FnMut(Vec<&'a Write>) -> Fut,
    Fut: std::future::Future<Output = QueueResult<Option<Vec<u64>>>>,
{
    // A stale key must not turn all unrelated work into serial individual
    // commits. Split only a batch the server definitively rejected. Every
    // child still contains complete logical operations, never half an enqueue.
    // Execute one child at a time to preserve this lane's concurrency bound.
    let mut pending = vec![groups];
    let mut outcomes = Vec::with_capacity(groups.len());
    while let Some(part) = pending.pop() {
        // One deadline bounds all conflict splitting. Preserve already confirmed
        // siblings instead of letting an outer timeout erase their outcomes.
        if tokio::time::Instant::now() >= deadline {
            outcomes.extend(groups[outcomes.len()..].iter().map(|_| {
                Err(error(
                    "batch deadline expired before this operation was attempted",
                ))
            }));
            break;
        }
        let writes: Vec<_> = part.iter().flat_map(|g| &g.writes).collect();
        match tokio::time::timeout_at(deadline, attempt(writes))
            .await
            .unwrap_or_else(|_| Err(error("timed out; commit outcome may be unknown")))
        {
            Ok(Some(revisions)) => {
                let mut offset = 0;
                for group in part {
                    let end = offset + group.writes.len();
                    outcomes.push(Ok(Some(revisions[offset..end].to_vec())));
                    offset = end;
                }
            }
            Ok(None) => {
                let _conflict =
                    crate::diagnostics::Scope::new(crate::diagnostics::NATS_BATCH_REJECTED);
                if part.len() == 1 {
                    outcomes.push(Ok(None));
                } else {
                    let (left, right) = part.split_at(part.len() / 2);
                    // Stack order preserves the input/reply ordering.
                    pending.push(right);
                    pending.push(left);
                }
            }
            Err(e) => {
                // An uncertain outcome is never split or replayed. Independent
                // parts rejected earlier may still be processed safely.
                outcomes.extend(part.iter().map(|_| Err(e.clone())));
            }
        }
    }
    Ok(outcomes)
}

async fn atomic(
    context: &jetstream::Context,
    bucket: &kv::Store,
    writes: &[&Write],
) -> QueueResult<Option<Vec<u64>>> {
    if writes.is_empty() {
        return Err(error("cannot publish an empty atomic batch"));
    }
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
        let send_time = crate::diagnostics::start();
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
        crate::diagnostics::elapsed(crate::diagnostics::NATS_ATOMIC_SEND, send_time);
    }
    // Atomic batches have fixed bounds and no negotiated flow window. Every
    // frame requires API level 2; send them in connection order without an
    // extra staging round trip. (ADR-50 fast-ingest has different rules.)
    // Staging replies are still consumed, but only a final durable ack succeeds.
    let _wait = crate::diagnostics::Scope::new(crate::diagnostics::NATS_ATOMIC_FINAL_WAIT);
    loop {
        let response = replies
            .next()
            .await
            .ok_or_else(|| error("acknowledgement stream closed; outcome may be unknown"))?;
        if response.status.is_some_and(|status| !status.is_success()) {
            return Err(error("server rejected batch request"));
        }
        if response.payload.is_empty() {
            continue;
        }
        match serde_json::from_slice::<Response<PublishAck>>(&response.payload).map_err(error)? {
            Response::Err { error: e } if conflict(&e) => return Ok(None),
            Response::Err { error: e } => return Err(error(e)),
            Response::Ok(ack) => {
                if ack.stream != bucket.stream_name
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        backend::{nats::NatsBackend, QueueBackend},
        JobMessage, QueueCtx,
    };
    use std::sync::Arc;
    fn groups(count: usize) -> Vec<Group> {
        (0..count)
            .map(|n| Group {
                queued: None,
                writes: vec![
                    write(&format!("metadata-{n}"), 0, vec![n as u8 * 2]),
                    write(&format!("payload-{n}"), 0, vec![n as u8 * 2 + 1]),
                ],
                reply: oneshot::channel().0,
            })
            .collect()
    }
    #[tokio::test]
    async fn one_stale_group_does_not_serialize_every_other_job() {
        let groups = groups(64);
        let mut calls = Vec::new();
        let outcomes = execute_groups(&groups, |writes| {
            let keys: Vec<_> = writes.iter().map(|w| w.key.as_str()).collect();
            assert_eq!(writes.len() % 2, 0, "never split an enqueue pair");
            calls.push(keys.clone());
            std::future::ready(Ok(if keys.contains(&"payload-29") {
                None
            } else {
                Some(writes.iter().map(|w| u64::from(w.value[0])).collect())
            }))
        })
        .await
        .unwrap();
        assert!(calls.len() <= 13, "one stale job caused linear retries");
        for (n, outcome) in outcomes.into_iter().enumerate() {
            assert_eq!(
                outcome.unwrap(),
                (n != 29).then_some(vec![n as u64 * 2, n as u64 * 2 + 1])
            );
        }
    }
    #[tokio::test]
    async fn uncertain_child_commit_is_not_split_or_replayed() {
        let groups = groups(16);
        let mut uncertain_attempts = 0;
        let outcomes = execute_groups(&groups, |writes| {
            let stale = writes.iter().any(|w| w.key == "payload-3");
            let uncertain = writes.iter().any(|w| w.key == "payload-8");
            std::future::ready(if stale {
                Ok(None) // This rejection establishes that nothing committed.
            } else if uncertain {
                uncertain_attempts += 1;
                Err(error("acknowledgement lost; commit outcome may be unknown"))
            } else {
                Ok(Some(writes.iter().map(|w| u64::from(w.value[0])).collect()))
            })
        })
        .await
        .unwrap();
        assert_eq!(uncertain_attempts, 1);
        for (n, outcome) in outcomes.into_iter().enumerate() {
            if n >= 8 {
                assert!(outcome.is_err());
            } else {
                assert_eq!(
                    outcome.unwrap(),
                    (n != 3).then_some(vec![n as u64 * 2, n as u64 * 2 + 1])
                );
            }
        }
    }
    #[tokio::test]
    async fn timeout_preserves_confirmed_siblings_and_does_not_attempt_pending_parts() {
        let groups = groups(4);
        let mut calls = Vec::new();
        let outcomes = execute_groups_until(
            &groups,
            tokio::time::Instant::now() + Duration::from_millis(100),
            |writes| {
                let keys: Vec<_> = writes.iter().map(|w| w.key.clone()).collect();
                calls.push(keys.clone());
                async move {
                    if writes.len() > 2 {
                        Ok(None) // Definitively rejected; splitting is safe.
                    } else if keys[0] == "metadata-0" {
                        Ok(Some(vec![41, 42]))
                    } else {
                        // This operation was submitted, but its acknowledgement
                        // never arrives. It must not erase the confirmed sibling.
                        std::future::pending().await
                    }
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(calls.len(), 4); // Root, left half, first job, second job.
        assert_eq!(outcomes.len(), 4);
        assert_eq!(outcomes[0].as_ref().unwrap(), &Some(vec![41, 42]));
        assert!(outcomes[1]
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("outcome may be unknown"));
        for outcome in &outcomes[2..] {
            assert!(outcome
                .as_ref()
                .unwrap_err()
                .to_string()
                .contains("before this operation was attempted"));
        }
        assert!(calls
            .iter()
            .skip(1)
            .all(|keys| !keys.iter().any(|key| key == "metadata-2")));
    }
    #[tokio::test]
    async fn expired_batch_deadline_does_not_start_a_commit() {
        let groups = groups(2);
        let outcomes = execute_groups_until(&groups, tokio::time::Instant::now(), |_| {
            panic!("an expired batch must not start a new commit");
            #[allow(unreachable_code)]
            std::future::ready(Ok(None))
        })
        .await
        .unwrap();
        assert_eq!(outcomes.len(), 2);
        assert!(outcomes.iter().all(|outcome| outcome
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("before this operation was attempted")));
    }
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
    async fn frames_pipeline_without_treating_staging_as_commit() {
        let (js, mut bucket) = fixture().await;
        let client = js.client();
        // Intercept protocol frames on a private core subject. Withhold staging
        // until every frame arrives, then withhold final commit separately.
        bucket.prefix = format!("dogrs.protocol.{}.", uuid::Uuid::new_v4().simple());
        bucket.put_prefix = None;
        bucket.use_jetstream_prefix = false;
        let mut frames = client
            .subscribe(format!("{}>", bucket.prefix))
            .await
            .unwrap();
        client.flush().await.unwrap();
        let mut task = {
            let js = js.clone();
            let bucket = bucket.clone();
            tokio::spawn(async move {
                let writes = [
                    write("job", 0, vec![1]),
                    write("payload", 0, vec![2; 65536]),
                ];
                atomic(&js, &bucket, &writes.iter().collect::<Vec<_>>()).await
            })
        };
        let first = tokio::time::timeout(Duration::from_secs(2), frames.next())
            .await
            .unwrap()
            .unwrap();
        let last = tokio::time::timeout(Duration::from_secs(2), frames.next())
            .await
            .expect("batch waited for staging before sending its remaining frames")
            .unwrap();
        assert_eq!(
            first
                .headers
                .as_ref()
                .unwrap()
                .get("Nats-Batch-Sequence")
                .unwrap()
                .as_str(),
            "1"
        );
        assert_eq!(
            last.headers
                .as_ref()
                .unwrap()
                .get("Nats-Batch-Sequence")
                .unwrap()
                .as_str(),
            "2"
        );
        assert_eq!(
            last.headers
                .as_ref()
                .unwrap()
                .get("Nats-Batch-Commit")
                .unwrap()
                .as_str(),
            "1"
        );
        for frame in [&first, &last] {
            assert_eq!(
                frame
                    .headers
                    .as_ref()
                    .unwrap()
                    .get("Nats-Required-Api-Level")
                    .unwrap()
                    .as_str(),
                "2"
            );
        }
        let reply = first.reply.unwrap();
        client
            .publish(reply.clone(), Vec::new().into())
            .await
            .unwrap();
        client.flush().await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut task)
                .await
                .is_err(),
            "staging acknowledgement must not establish a successful commit"
        );
        let ack = serde_json::json!({"stream":bucket.stream_name,"seq":2,"batch":last.headers.as_ref().unwrap().get("Nats-Batch-Id").unwrap().as_str(),"count":2});
        client
            .publish(reply, serde_json::to_vec(&ack).unwrap().into())
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            Some(vec![1, 2])
        );
        js.delete_key_value(&bucket.name).await.unwrap();
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
        // The coalescing target must not reject or split a valid large job.
        let writer = BatchWriter::start(js.clone(), bucket.clone());
        let metadata_revision = writer
            .enqueue(
                "large-payload".into(),
                vec![5; 512 * 1024],
                "large-job",
                vec![6; 1024],
                0,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            bucket.get("large-payload").await.unwrap().unwrap().as_ref(),
            &vec![5; 512 * 1024]
        );
        assert_eq!(
            bucket.get("large-job").await.unwrap().unwrap().as_ref(),
            &vec![6; 1024]
        );
        assert_eq!(
            bucket.entry("large-job").await.unwrap().unwrap().revision,
            metadata_revision,
            "enqueue must return the metadata revision, not the final payload revision"
        );
        assert!(writer
            .submit("large-job", vec![7], metadata_revision)
            .await
            .unwrap()
            .is_some());
        // Rejection of the final payload write must roll back the preceding
        // metadata, so discovery never exposes a job without its own payload.
        assert!(writer
            .enqueue(
                "large-payload".into(),
                vec![9],
                "uncommitted-job",
                vec![8],
                0
            )
            .await
            .unwrap()
            .is_none());
        assert!(bucket.get("uncommitted-job").await.unwrap().is_none());
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
        for _ in 0..ADMISSION_CAPACITY {
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
        for n in 0..128u8 {
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
        // Race the first submission of a scope across independent writers and
        // indexes, not just duplicate lookups of an already committed job.
        let peer_context = jetstream::new(
            async_nats::connect(std::env::var("DOGRS_NATS_URL").unwrap())
                .await
                .unwrap(),
        );
        let peer = Arc::new(
            NatsBackend::from_context(peer_context, &bucket.name, 1024 * 1024)
                .await
                .unwrap(),
        );
        peer.get_snapshot(QueueCtx::new("t"), ids.iter().next().unwrap().clone())
            .await
            .unwrap();
        let barrier = Arc::new(tokio::sync::Barrier::new(16));
        for n in 0..16 {
            let backend = if n % 2 == 0 {
                backend.clone()
            } else {
                peer.clone()
            };
            let barrier = barrier.clone();
            tasks.spawn(async move {
                barrier.wait().await;
                backend
                    .enqueue(
                        QueueCtx::new("t"),
                        JobMessage::new("work", vec![128; 65536], "bytes", "q")
                            .with_idempotency_key("fresh-race"),
                    )
                    .await
                    .unwrap()
            });
        }
        let mut winners = HashSet::new();
        while let Some(id) = tasks.join_next().await {
            winners.insert(id.unwrap());
        }
        assert_eq!(
            winners.len(),
            1,
            "only one fresh scope may win across writers"
        );
        ids.extend(winners);
        let mut keys = bucket.keys().await.unwrap();
        let mut payloads = 0;
        while let Some(key) = keys.next().await {
            if key.unwrap().starts_with("p.") {
                payloads += 1;
            }
        }
        assert_eq!(
            payloads, 129,
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
