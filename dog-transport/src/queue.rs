//! Queue snapshots over SSE; authenticate and authorize before constructing.
use crate::realtime::SseEvent;
use dog_queue::{
    realtime::{watch_jobs, WatchOptions},
    JobId, QueueBackend, QueueCtx, QueueResult,
};
use futures_util::{Stream, StreamExt};
use std::sync::Arc;
use tokio::sync::watch;

/// A fresh connection sends current status for every authorized job, then changes.
/// No payloads, results or raw errors are exposed. Dropping the stream stops reads.
/// Supply a revocation receiver for private sessions; true or sender loss closes it.
/// Limit `options.lifetime` to the remaining credential lifetime before calling.
/// Admission limits, heartbeat, and HTTP write deadlines belong to the host.
pub fn queue_sse_stream(
    backend: Arc<dyn QueueBackend>,
    ctx: QueueCtx,
    jobs: Vec<JobId>,
    options: WatchOptions,
    revoke: watch::Receiver<bool>,
) -> QueueResult<impl Stream<Item = SseEvent> + Send> {
    let updates = watch_jobs(backend, ctx, jobs, options)?;
    Ok(futures_util::stream::unfold(
        (updates, revoke, false),
        |(mut updates, mut revoke, done)| async move {
            if done {
                return None;
            }
            let result = tokio::select! {
                biased;
                _ = async {
                    loop {
                        if *revoke.borrow_and_update() { break; }
                        if revoke.changed().await.is_err() { break; }
                    }
                } => Err("revoked"),
                update = updates.next() => match update {
                    Some(Ok(update)) => serde_json::to_string(&update).map_err(|_| "encoding_failed"),
                    Some(Err(_)) => Err("resync_required"),
                    None => Err("expired"),
                },
            };
            let (event, done) = match result {
                Ok(data) => (
                    SseEvent {
                        event: Some("dogrs.job_status"),
                        data,
                    },
                    false,
                ),
                Err(reason) => (
                    SseEvent {
                        event: Some("dogrs.stream_error"),
                        data: serde_json::json!({"reason": reason, "resync_required": true})
                            .to_string(),
                    },
                    true,
                ),
            };
            Some((event, (updates, revoke, done)))
        },
    ))
}
