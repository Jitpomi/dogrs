//! Authoritative, tenant-scoped status snapshots for reconnecting clients.
//!
//! Unlike backend event streams this works across processes and durable adapters.
//! It coalesces intermediate transitions; it is not an event log or replay protocol.
use crate::{backend::BoxStream, JobId, QueueBackend, QueueCtx, QueueError, QueueResult};
use chrono::{DateTime, Utc};
use futures::stream;
use serde::Serialize;
use std::{collections::HashMap, sync::Arc, time::Duration};

/// Status and progress summary. Results require explicit opt-in authorization.
/// Payloads, raw errors, trace context and lease tokens stay private.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobUpdate {
    pub job_id: JobId,
    pub status: String,
    pub attempt: u32,
    pub progress: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct WatchOptions {
    pub interval: Duration,
    pub read_timeout: Duration,
    pub lifetime: Duration,
    /// Opt in only after authorizing access to results as well as status.
    pub include_results: bool,
}
impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(1),
            read_timeout: Duration::from_secs(10),
            lifetime: Duration::from_secs(3600),
            include_results: false,
        }
    }
}

/// Watch an explicit, authorized set of jobs. A fresh subscription always sends
/// current snapshots, including jobs completed while the client was disconnected.
///
/// The caller MUST authenticate `ctx` and authorize every job ID; tenant scope
/// alone does not establish user-level access. Revoke by dropping this stream.
/// Reads are sequential and demand-driven, with no detached task or unbounded
/// buffer. A backend error/timeout ends the stream after one error; the host must
/// signal resynchronization and must not expose raw backend errors to clients.
/// The lifetime ends the stream and requires reauthorization on reconnect.
pub fn watch_jobs(
    backend: Arc<dyn QueueBackend>,
    ctx: QueueCtx,
    mut jobs: Vec<JobId>,
    options: WatchOptions,
) -> QueueResult<BoxStream<QueueResult<JobUpdate>>> {
    if ctx.tenant_id.is_empty()
        || jobs.is_empty()
        || jobs.len() > 128
        || options.interval < Duration::from_millis(100)
        || options.interval > Duration::from_secs(60)
        || options.read_timeout.is_zero()
        || options.read_timeout > Duration::from_secs(60)
        || options.lifetime.is_zero()
        || options.lifetime > Duration::from_secs(86400)
    {
        return Err(QueueError::InvalidConfig("watch requires a tenant, 1..=128 jobs, interval 100ms..=60s, read timeout 0..=60s, lifetime 0..=24h".into()));
    }
    let mut seen = std::collections::HashSet::new();
    jobs.retain(|id| seen.insert(id.clone()));
    let deadline = tokio::time::Instant::now() + options.lifetime;
    let state = (
        backend,
        ctx,
        jobs,
        options,
        HashMap::<JobId, JobUpdate>::new(),
        0usize,
        deadline,
        false,
    );
    Ok(Box::pin(stream::unfold(
        state,
        |(backend, ctx, jobs, options, mut previous, mut index, deadline, mut done)| async move {
            if done {
                return None;
            }
            loop {
                if tokio::time::Instant::now() >= deadline {
                    return None;
                }
                if index == jobs.len() {
                    tokio::time::sleep_until(
                        (tokio::time::Instant::now() + options.interval).min(deadline),
                    )
                    .await;
                    index = 0;
                    continue;
                }
                let id = jobs[index].clone();
                index += 1;
                let read_deadline =
                    (tokio::time::Instant::now() + options.read_timeout).min(deadline);
                let result = tokio::time::timeout_at(
                    read_deadline,
                    backend.get_snapshot(ctx.clone(), id.clone()),
                )
                .await;
                let item = match result {
                    Ok(Ok(snapshot)) => {
                        // Fail closed even if a custom backend violates its scope contract.
                        if snapshot.tenant_id != ctx.tenant_id || snapshot.job_id != id {
                            done = true;
                            Err(QueueError::Internal("snapshot scope mismatch".into()))
                        } else if options.include_results
                            && snapshot.result.as_ref().is_some_and(|r| r.len() > 4096)
                        {
                            done = true;
                            Err(QueueError::InvalidConfig(
                                "streamed result exceeds 4 KiB; use an authorized result reference"
                                    .into(),
                            ))
                        } else {
                            let update = JobUpdate {
                                job_id: id.clone(),
                                status: snapshot.status.name().to_owned(),
                                attempt: snapshot.attempt,
                                progress: snapshot.progress,
                                result: if options.include_results
                                    && matches!(snapshot.status, crate::JobStatus::Completed { .. })
                                {
                                    snapshot.result
                                } else {
                                    None
                                },
                                updated_at: snapshot.updated_at,
                            };
                            if previous.get(&id) == Some(&update) {
                                continue;
                            }
                            previous.insert(id, update.clone());
                            Ok(update)
                        }
                    }
                    Ok(Err(error)) => {
                        done = true;
                        Err(error)
                    }
                    Err(_) => {
                        done = true;
                        Err(QueueError::Internal(
                            "status read timed out; reconnect and resynchronize".into(),
                        ))
                    }
                };
                return Some((
                    item,
                    (backend, ctx, jobs, options, previous, index, deadline, done),
                ));
            }
        },
    )))
}

// Scoped to handler polls: concurrent jobs cannot overwrite each other's reporter.
tokio::task_local! {
    pub(crate) static REPORTER: ProgressReporter;
}
pub(crate) struct ProgressReporter {
    pub backend: Arc<dyn QueueBackend>,
    pub ctx: QueueCtx,
    pub id: JobId,
    pub token: crate::LeaseToken,
}
/// Report progress from a QueueAdapter handler. Await the result; failures can
/// indicate loss of the lease. Does not extend the lease or mark completion.
/// Spawned tasks do not inherit this scope. Throttle updates for durable storage.
pub async fn report_progress(percent: u8) -> QueueResult<()> {
    let (backend, ctx, id, token) = REPORTER
        .try_with(|r| {
            (
                r.backend.clone(),
                r.ctx.clone(),
                r.id.clone(),
                r.token.clone(),
            )
        })
        .map_err(|_| {
            QueueError::InvalidConfig("progress requires an active queue handler".into())
        })?;
    backend.report_progress(ctx, id, token, percent).await
}
