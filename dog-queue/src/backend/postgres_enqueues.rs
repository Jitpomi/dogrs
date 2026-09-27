//! Bound concurrent producer writes and share commits without sharing identities.
use super::{error, metadata, Manager, Operation, TenantState};
use crate::{JobId, JobMessage, QueueResult};
use chrono::Utc;
use std::{collections::HashSet, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tokio_postgres::types::Type;

struct Request {
    tenant: String,
    id: JobId,
    state: serde_json::Value,
    message: JobMessage,
    response: oneshot::Sender<QueueResult<JobId>>,
}
impl Request {
    fn new(
        tenant: &str,
        message: &JobMessage,
        response: oneshot::Sender<QueueResult<JobId>>,
    ) -> QueueResult<Self> {
        let mut state = TenantState::default();
        state.apply_at(
            tenant,
            &Operation::Enqueue(super::super::durable::metadata_message(message)),
            Utc::now(),
        )?;
        let stored = state.jobs.values().next().unwrap();
        Ok(Self {
            tenant: tenant.into(),
            id: stored.record.job_id.clone(),
            state: metadata(stored)?,
            message: message.clone(),
            response,
        })
    }
    fn scope(&self) -> Option<(&str, &str, &str, &str)> {
        self.message.idempotency_key.as_deref().map(|key| {
            (
                self.tenant.as_str(),
                self.message.queue.as_str(),
                self.message.job_type.as_str(),
                key,
            )
        })
    }
}
pub(super) struct Enqueues(mpsc::Sender<Request>);
impl Enqueues {
    pub(super) fn start(
        pool: bb8::Pool<Manager>,
        concurrency: usize,
        batch_size: usize,
        timeout: Duration,
    ) -> Self {
        let (sender, mut receiver) = mpsc::channel::<Request>(256);
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
                let mut bytes = first.message.payload_bytes.len();
                let mut scopes = HashSet::new();
                if let Some(scope) = first.scope() {
                    scopes.insert((
                        scope.0.to_owned(),
                        scope.1.to_owned(),
                        scope.2.to_owned(),
                        scope.3.to_owned(),
                    ));
                }
                let mut requests = vec![first];
                while requests.len() < batch_size && bytes < 512 * 1024 {
                    let Ok(request) = receiver.try_recv() else {
                        break;
                    };
                    let size = request.message.payload_bytes.len();
                    let duplicate = request.scope().is_some_and(|s| {
                        !scopes.insert((
                            s.0.to_owned(),
                            s.1.to_owned(),
                            s.2.to_owned(),
                            s.3.to_owned(),
                        ))
                    });
                    // PostgreSQL cannot update the same conflict row twice in
                    // one INSERT. Preserve independent calls and bound payloads.
                    if duplicate || size > 512 * 1024 - bytes {
                        deferred = Some(request);
                        break;
                    }
                    bytes += size;
                    requests.push(request);
                }
                let pool = pool.clone();
                running.spawn(async move {
                    requests.retain(|r| !r.response.is_closed());
                    if requests.is_empty() {
                        return;
                    }
                    let outcomes = tokio::time::timeout(timeout, execute(&pool, &requests))
                        .await
                        .unwrap_or_else(|_| {
                            Err(error(
                                "PostgreSQL enqueue batch timed out; commit outcome may be unknown",
                            ))
                        });
                    match outcomes {
                        Ok(outcomes) => {
                            for (request, outcome) in requests.into_iter().zip(outcomes) {
                                let _ = request.response.send(outcome);
                            }
                        }
                        Err(e) => {
                            for request in requests {
                                let _ = request.response.send(Err(e.clone()));
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
    pub(super) async fn submit(&self, tenant: &str, message: &JobMessage) -> QueueResult<JobId> {
        let (response, receiver) = oneshot::channel();
        self.0
            .send(Request::new(tenant, message, response)?)
            .await
            .map_err(|_| error("PostgreSQL enqueue dispatcher closed"))?;
        receiver
            .await
            .map_err(|_| error("PostgreSQL enqueue interrupted; commit outcome may be unknown"))?
    }
}
async fn execute(
    pool: &bb8::Pool<Manager>,
    requests: &[Request],
) -> QueueResult<Vec<QueueResult<JobId>>> {
    let tenants: Vec<_> = requests.iter().map(|r| r.tenant.as_str()).collect();
    let ids: Vec<_> = requests.iter().map(|r| r.id.as_str()).collect();
    let states: Vec<_> = requests.iter().map(|r| &r.state).collect();
    let queues: Vec<_> = requests.iter().map(|r| r.message.queue.as_str()).collect();
    let kinds: Vec<_> = requests
        .iter()
        .map(|r| r.message.job_type.as_str())
        .collect();
    let dedupes: Vec<_> = requests
        .iter()
        .map(|r| r.message.idempotency_key.as_deref())
        .collect();
    let priorities: Vec<_> = requests
        .iter()
        .map(|r| i32::from(r.message.priority.as_u8()))
        .collect();
    let schedules: Vec<_> = requests.iter().map(|r| r.message.run_at).collect();
    let payloads: Vec<_> = requests
        .iter()
        .map(|r| r.message.payload_bytes.as_slice())
        .collect();
    let client = pool.get().await.map_err(error)?;
    let mut attempts = 0;
    let rows = loop {
        match client
            .query_typed(
                include_str!("postgres_enqueue_batch.sql"),
                &[
                    (&tenants, Type::TEXT_ARRAY),
                    (&ids, Type::TEXT_ARRAY),
                    (&states, Type::JSONB_ARRAY),
                    (&queues, Type::TEXT_ARRAY),
                    (&kinds, Type::TEXT_ARRAY),
                    (&dedupes, Type::TEXT_ARRAY),
                    (&priorities, Type::INT4_ARRAY),
                    (&schedules, Type::TIMESTAMPTZ_ARRAY),
                    (&payloads, Type::BYTEA_ARRAY),
                ],
            )
            .await
        {
            Ok(rows) => break rows,
            Err(e)
                if attempts < 2
                    && e.as_db_error()
                        .is_some_and(|e| matches!(e.code().code(), "40P01" | "40001")) =>
            {
                attempts += 1;
                tokio::time::sleep(Duration::from_millis(attempts * 5)).await;
            }
            // A data/constraint error aborts the entire autocommit statement.
            // Isolate its requests so one invalid timestamp/index value cannot
            // reject unrelated callers. Never replay an unknown commit outcome.
            Err(e)
                if requests.len() > 1
                    && e.as_db_error().is_some_and(|e| {
                        e.code().code().starts_with("22")
                            || e.code().code().starts_with("23")
                            || e.code().code() == "54000"
                    }) =>
            {
                drop(client);
                let mut outcomes = Vec::with_capacity(requests.len());
                for request in requests {
                    outcomes.push(
                        match Box::pin(execute(pool, std::slice::from_ref(request))).await {
                            Ok(mut outcomes) => outcomes.remove(0),
                            Err(e) => Err(e),
                        },
                    );
                }
                return Ok(outcomes);
            }
            Err(e) => return Err(error(e)),
        }
    };
    if rows.len() != requests.len() {
        return Err(error(
            "PostgreSQL enqueue batch returned incomplete results; commit outcome may be unknown",
        ));
    }
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            if row.get::<_, i64>(0) != index as i64 + 1 {
                return Err(error("PostgreSQL enqueue batch returned invalid ordinal"));
            }
            Ok(Ok(row.get::<_, String>(1).into()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        backend::postgres::{PostgresBackend, PostgresConfig},
        QueueBackend, QueueCtx,
    };

    #[tokio::test]
    #[ignore = "requires disposable PostgreSQL"]
    async fn aborted_invalid_enqueue_batch_isolates_the_bad_request() {
        let backend = PostgresBackend::new(PostgresConfig {
            connection_string: std::env::var("DOGRS_POSTGRES_URL").unwrap(),
        })
        .await
        .unwrap();
        for bad in [
            JobMessage::new("bad-date", vec![1], "bytes", "q")
                .with_run_at(chrono::DateTime::<Utc>::MIN_UTC),
            JobMessage::new("bad-index", vec![1], "bytes", "q").with_idempotency_key(
                (0..150)
                    .map(|_| uuid::Uuid::new_v4().to_string())
                    .collect::<String>(),
            ),
        ] {
            let tenant = format!("batch-invalid-{}", uuid::Uuid::new_v4());
            let messages = [
                JobMessage::new("first", vec![4; 65536], "bytes", "q"),
                bad,
                JobMessage::new("last", vec![8; 65536], "bytes", "q"),
            ];
            let requests: Vec<_> = messages
                .iter()
                .map(|message| Request::new(&tenant, message, oneshot::channel().0).unwrap())
                .collect();
            let outcomes = execute(&backend.store.pool, &requests).await.unwrap();
            assert!(outcomes[0].is_ok());
            assert!(outcomes[1].is_err());
            assert!(outcomes[2].is_ok());
            for n in [0, 2] {
                let row = backend
                    .get_record(QueueCtx::new(&tenant), outcomes[n].clone().unwrap())
                    .await
                    .unwrap();
                assert_eq!(row.message.payload_bytes, messages[n].payload_bytes);
            }
            assert!(matches!(
                backend
                    .get_record(QueueCtx::new(&tenant), requests[1].id.clone())
                    .await,
                Err(crate::QueueError::JobNotFound(_))
            ));
        }
    }
}
