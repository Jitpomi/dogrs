//! Combine concurrent claims from distinct tenants without sharing ownership.
use super::{error, Manager, StoredRecord};
use crate::{LeaseToken, LeasedJob, QueueResult};
use std::{collections::HashSet, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tokio_postgres::types::Type;

struct Request {
    queued_at: Option<std::time::Instant>,
    tenant: String,
    queues: serde_json::Value,
    token: LeaseToken,
    seconds: f64,
    response: oneshot::Sender<QueueResult<Option<LeasedJob>>>,
}
pub(super) struct Claims(mpsc::Sender<Request>);
impl Claims {
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
                let mut tenants = HashSet::new();
                tenants.insert(first.tenant.clone());
                let mut requests = vec![first];
                while requests.len() < batch_size {
                    let Ok(request) = receiver.try_recv() else {
                        break;
                    };
                    // SKIP LOCKED does not skip a row locked by this same SQL
                    // statement. Keep overlapping tenant requests separate so
                    // two callers cannot choose the same row inside one batch.
                    if !tenants.insert(request.tenant.clone()) {
                        deferred = Some(request);
                        break;
                    }
                    requests.push(request);
                }
                let pool = pool.clone();
                running.spawn(async move {
                    for request in &requests { crate::diagnostics::elapsed(crate::diagnostics::PG_CLAIM_QUEUE, request.queued_at); }
                    requests.retain(|r| !r.response.is_closed());
                    if requests.is_empty() {
                        return;
                    }
                    let outcomes = tokio::time::timeout(timeout, execute(&pool, &requests))
                        .await
                        .unwrap_or_else(|_| {
                            Err(error("PostgreSQL claim batch timed out; ownership outcome may be unknown"))
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
    pub(super) async fn submit(
        &self,
        tenant: &str,
        queues: &[String],
        duration: Duration,
    ) -> QueueResult<Option<LeasedJob>> {
        let (response, receiver) = oneshot::channel();
        self.0
            .send(Request {
                queued_at: crate::diagnostics::start(),
                tenant: tenant.into(),
                queues: serde_json::to_value(queues).map_err(error)?,
                token: LeaseToken::new(),
                seconds: duration.as_secs_f64(),
                response,
            })
            .await
            .map_err(|_| error("PostgreSQL claim dispatcher closed"))?;
        receiver
            .await
            .map_err(|_| error("PostgreSQL claim interrupted; ownership outcome may be unknown"))?
    }
}
async fn execute(
    pool: &bb8::Pool<Manager>,
    requests: &[Request],
) -> QueueResult<Vec<QueueResult<Option<LeasedJob>>>> {
    let tenants: Vec<&str> = requests.iter().map(|r| r.tenant.as_str()).collect();
    let queues: Vec<&serde_json::Value> = requests.iter().map(|r| &r.queues).collect();
    let tokens: Vec<&str> = requests.iter().map(|r| r.token.as_str()).collect();
    let durations: Vec<f64> = requests.iter().map(|r| r.seconds).collect();
    let client = crate::diagnostics::measure(crate::diagnostics::PG_CLAIM_POOL, pool.get())
        .await
        .map_err(error)?;
    let rows = crate::diagnostics::measure(
        crate::diagnostics::PG_CLAIM_SQL,
        client.query_typed(
            include_str!("postgres_claim_batch.sql"),
            &[
                (&tenants, Type::TEXT_ARRAY),
                (&queues, Type::JSONB_ARRAY),
                (&tokens, Type::TEXT_ARRAY),
                (&durations, Type::FLOAT8_ARRAY),
            ],
        ),
    )
    .await
    .map_err(error)?;
    if rows.len() != requests.len() {
        return Err(error(
            "PostgreSQL claim batch returned incomplete results; ownership outcome may be unknown",
        ));
    }
    let _decode = crate::diagnostics::Scope::new(crate::diagnostics::PG_CLAIM_DECODE);
    Ok(rows
        .into_iter()
        .zip(requests)
        .enumerate()
        .map(|(index, (row, request))| {
            if row.get::<_, i64>(0) != index as i64 + 1 {
                return Err(error("PostgreSQL claim batch returned an invalid ordinal"));
            }
            let Some(value) = row.get::<_, Option<serde_json::Value>>(1) else {
                return Ok(None);
            };
            let mut stored: StoredRecord = serde_json::from_value(value).map_err(error)?;
            if let Some(payload) = row.get::<_, Option<Vec<u8>>>(2) {
                stored.record.message.payload_bytes = payload;
            }
            let until = stored
                .record
                .lease_until()
                .ok_or_else(|| error("PostgreSQL claim did not return a lease"))?;
            stored.record.lease_token = Some(request.token.clone());
            Ok(Some(LeasedJob::new(
                stored.record,
                request.token.clone(),
                until,
            )))
        })
        .collect())
}
