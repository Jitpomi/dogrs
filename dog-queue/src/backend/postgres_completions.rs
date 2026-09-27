//! Coalesce only concurrent completions, without weakening their commit boundary.
use super::{error, Manager};
use crate::{JobId, LeaseToken, QueueError, QueueResult};
use std::{collections::HashSet, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tokio_postgres::types::Type;

struct Request {
    queued_at: Option<std::time::Instant>,
    tenant: String,
    id: JobId,
    token: LeaseToken,
    result: Option<String>,
    response: oneshot::Sender<QueueResult<()>>,
}
pub(super) struct Completions(mpsc::Sender<Request>);
impl Completions {
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
                // No timer is imposed on a quiet queue. Under load, requests
                // accumulate while the bounded set of previous commits runs.
                tokio::task::yield_now().await;
                let mut keys = HashSet::new();
                keys.insert((first.tenant.clone(), first.id.clone()));
                let mut requests = vec![first];
                while requests.len() < batch_size {
                    let Ok(request) = receiver.try_recv() else {
                        break;
                    };
                    // Two completions of the same job must not both succeed
                    // from the same pre-update snapshot.
                    if !keys.insert((request.tenant.clone(), request.id.clone())) {
                        deferred = Some(request);
                        break;
                    }
                    requests.push(request);
                }
                let pool = pool.clone();
                running.spawn(async move {
                    for request in &requests { crate::diagnostics::elapsed(crate::diagnostics::PG_COMPLETE_QUEUE, request.queued_at); }
                    requests.retain(|r| !r.response.is_closed());
                    if requests.is_empty() { return; }
                    let outcomes = tokio::time::timeout(timeout, execute(&pool, &requests))
                        .await.unwrap_or_else(|_| Err(error("PostgreSQL completion batch timed out; commit outcome may be unknown")));
                    match outcomes {
                        Ok(outcomes) => {
                            for (request, outcome) in requests.into_iter().zip(outcomes) {
                                let _ = request.response.send(outcome);
                            }
                        }
                        Err(e) => {
                            for request in requests { let _ = request.response.send(Err(e.clone())); }
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
        tenant: &str,
        id: &JobId,
        token: &LeaseToken,
        result: &Option<String>,
    ) -> QueueResult<()> {
        let (response, receiver) = oneshot::channel();
        self.0
            .send(Request {
                queued_at: crate::diagnostics::start(),
                tenant: tenant.into(),
                id: id.clone(),
                token: token.clone(),
                result: result.clone(),
                response,
            })
            .await
            .map_err(|_| error("PostgreSQL completion dispatcher closed"))?;
        receiver.await.map_err(|_| {
            error("PostgreSQL completion interrupted; commit outcome may be unknown")
        })?
    }
}
async fn execute(
    pool: &bb8::Pool<Manager>,
    requests: &[Request],
) -> QueueResult<Vec<QueueResult<()>>> {
    let tenants: Vec<&str> = requests.iter().map(|r| r.tenant.as_str()).collect();
    let ids: Vec<&str> = requests.iter().map(|r| r.id.as_str()).collect();
    let tokens: Vec<&str> = requests.iter().map(|r| r.token.as_str()).collect();
    let results: Vec<Option<&str>> = requests.iter().map(|r| r.result.as_deref()).collect();
    let client = crate::diagnostics::measure(crate::diagnostics::PG_COMPLETE_POOL, pool.get())
        .await
        .map_err(error)?;
    // PostgreSQL explicitly rolls back an autocommit statement on deadlock or
    // serialization failure. Only those known-aborted outcomes may be retried;
    // transport errors and timeouts keep their unknown-commit semantics.
    let mut attempts = 0;
    let rows = loop {
        match crate::diagnostics::measure(
            crate::diagnostics::PG_COMPLETE_SQL,
            client.query_typed(
                include_str!("postgres_complete_batch.sql"),
                &[
                    (&tenants, Type::TEXT_ARRAY),
                    (&ids, Type::TEXT_ARRAY),
                    (&tokens, Type::TEXT_ARRAY),
                    (&results, Type::TEXT_ARRAY),
                ],
            ),
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
            Err(e) => return Err(error(e)),
        }
    };
    if rows.len() != requests.len() {
        return Err(error("PostgreSQL completion batch returned an incomplete result; commit outcome may be unknown"));
    }
    rows.into_iter()
        .zip(requests)
        .enumerate()
        .map(|(index, (row, request))| {
            if row.get::<_, i64>(0) != index as i64 + 1 {
                return Err(error(
                    "PostgreSQL completion batch returned an invalid ordinal",
                ));
            }
            Ok(match row.get::<_, i32>(1) {
                0 if row.get::<_, bool>(2) => Ok(()),
                1 => Err(QueueError::JobNotFound(request.id.clone())),
                2 => Err(QueueError::JobCanceled),
                3 => Err(QueueError::JobAlreadyTerminal),
                4 => Err(QueueError::InvalidLeaseToken {
                    job_id: request.id.clone(),
                }),
                5 => Err(QueueError::LeaseExpired),
                _ => Err(error("Unexpected PostgreSQL completion outcome")),
            })
        })
        .collect()
}
