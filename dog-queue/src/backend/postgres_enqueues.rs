//! Coalesce concurrent admissions while acknowledging only committed INSERTs.
use super::{error, Manager};
use crate::{JobId, JobMessage, QueueResult};
use std::{collections::HashSet, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tokio_postgres::types::Type;

struct Request {
    tenant: String,
    id: JobId,
    metadata: serde_json::Value,
    message: JobMessage,
    response: oneshot::Sender<QueueResult<JobId>>,
}
pub(super) struct Enqueues(mpsc::Sender<Request>);
impl Enqueues {
    pub(super) fn start(
        pool: bb8::Pool<Manager>,
        concurrency: usize,
        batch_size: usize,
        timeout: Duration,
    ) -> Self {
        let (sender, mut receiver) = mpsc::channel::<Request>(64);
        tokio::spawn(async move {
            let mut running = tokio::task::JoinSet::new();
            let mut deferred = None;
            loop {
                while running.len() >= concurrency {
                    let _ = running.join_next().await;
                }
                let first = match deferred.take() {
                    Some(request) => request,
                    None => match receiver.recv().await {
                        Some(request) => request,
                        None => break,
                    },
                };
                tokio::task::yield_now().await;
                let key = |r: &Request| {
                    r.message.idempotency_key.as_ref().map(|key| {
                        (
                            r.tenant.clone(),
                            r.message.queue.clone(),
                            r.message.job_type.clone(),
                            key.clone(),
                        )
                    })
                };
                let mut keys = HashSet::new();
                if let Some(key) = key(&first) {
                    keys.insert(key);
                }
                let mut requests = vec![first];
                while requests.len() < batch_size {
                    let Ok(request) = receiver.try_recv() else {
                        break;
                    };
                    // ON CONFLICT cannot update the same row twice in one INSERT.
                    if key(&request).is_some_and(|key| !keys.insert(key)) {
                        deferred = Some(request);
                        break;
                    }
                    requests.push(request);
                }
                let pool = pool.clone();
                running.spawn(async move {
                    requests.retain(|r| !r.response.is_closed());
                    if requests.is_empty() { return; }
                    let result = tokio::time::timeout(timeout, execute(&pool, &requests)).await
                        .unwrap_or_else(|_| Err(error("PostgreSQL enqueue batch timed out; commit outcome may be unknown; use idempotency keys")));
                    match result {
                        Ok(ids) => for (request, id) in requests.into_iter().zip(ids) { let _ = request.response.send(Ok(id)); },
                        Err(e) => for request in requests { let _ = request.response.send(Err(e.clone())); },
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
        tenant: &str,
        id: JobId,
        metadata: serde_json::Value,
        message: &JobMessage,
    ) -> QueueResult<JobId> {
        // Reserve before cloning a potentially large payload. The producer slot
        // budget additionally bounds outstanding requests across this dispatcher.
        let permit = self
            .0
            .reserve()
            .await
            .map_err(|_| error("PostgreSQL enqueue dispatcher closed"))?;
        let (response, receiver) = oneshot::channel();
        permit.send(Request {
            tenant: tenant.into(),
            id,
            metadata,
            message: message.clone(),
            response,
        });
        receiver
            .await
            .map_err(|_| error("PostgreSQL enqueue interrupted; commit outcome may be unknown"))?
    }
}
async fn execute(pool: &bb8::Pool<Manager>, requests: &[Request]) -> QueueResult<Vec<JobId>> {
    let tenants: Vec<&str> = requests.iter().map(|r| r.tenant.as_str()).collect();
    let ids: Vec<&str> = requests.iter().map(|r| r.id.as_str()).collect();
    let metadata: Vec<&serde_json::Value> = requests.iter().map(|r| &r.metadata).collect();
    let queues: Vec<&str> = requests.iter().map(|r| r.message.queue.as_str()).collect();
    let kinds: Vec<&str> = requests
        .iter()
        .map(|r| r.message.job_type.as_str())
        .collect();
    let dedupes: Vec<Option<&str>> = requests
        .iter()
        .map(|r| r.message.idempotency_key.as_deref())
        .collect();
    let priorities: Vec<i32> = requests
        .iter()
        .map(|r| i32::from(r.message.priority.as_u8()))
        .collect();
    let times: Vec<_> = requests.iter().map(|r| r.message.run_at).collect();
    let payloads: Vec<&[u8]> = requests
        .iter()
        .map(|r| r.message.payload_bytes.as_slice())
        .collect();
    let client = crate::diagnostics::measure(crate::diagnostics::PG_POOL, pool.get())
        .await
        .map_err(error)?;
    let statement = client
        .enqueue_batch
        .get_or_try_init(|| async {
            client
                .prepare_typed(
                    include_str!("postgres_enqueue_batch.sql"),
                    &[
                        Type::TEXT_ARRAY,
                        Type::TEXT_ARRAY,
                        Type::JSONB_ARRAY,
                        Type::TEXT_ARRAY,
                        Type::TEXT_ARRAY,
                        Type::TEXT_ARRAY,
                        Type::INT4_ARRAY,
                        Type::TIMESTAMPTZ_ARRAY,
                        Type::BYTEA_ARRAY,
                    ],
                )
                .await
        })
        .await
        .map_err(error)?;
    let mut attempts = 0;
    let rows = loop {
        match crate::diagnostics::measure(
            crate::diagnostics::PG_INSERT,
            client.query(
                statement,
                &[
                    &tenants,
                    &ids,
                    &metadata,
                    &queues,
                    &kinds,
                    &dedupes,
                    &priorities,
                    &times,
                    &payloads,
                ],
            ),
        )
        .await
        {
            Ok(rows) => break rows,
            // Retry only outcomes PostgreSQL explicitly guarantees were aborted.
            Err(e)
                if attempts < 2
                    && e.as_db_error()
                        .is_some_and(|e| matches!(e.code().code(), "40P01" | "40001")) =>
            {
                attempts += 1;
                tokio::time::sleep(Duration::from_millis(attempts * 5)).await;
            }
            Err(e) => return Err(error(e)),
        }
    };
    if rows.len() != requests.len() {
        return Err(error(
            "PostgreSQL enqueue batch returned incomplete results; commit outcome may be unknown",
        ));
    }
    rows.into_iter().enumerate().map(|(index, row)| {
        if row.get::<_, i64>(0) != index as i64 + 1 { return Err(error("PostgreSQL enqueue batch returned an invalid ordinal; commit outcome may be unknown")); }
        Ok(row.get::<_, String>(1).into())
    }).collect()
}
