//! Bounded optional enqueue coalescing. No request succeeds before its transaction commits.
use super::{enqueue_one, error, Manager, PreparedEnqueue};
use crate::{JobId, QueueError, QueueResult};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot, Mutex};

#[derive(Clone)]
pub struct PostgresBatchOptions {
    pub workers: u32,
    pub max_jobs: usize,
    pub max_delay: Duration,
}
impl Default for PostgresBatchOptions {
    fn default() -> Self {
        Self {
            workers: 2,
            max_jobs: 32,
            max_delay: Duration::from_millis(2),
        }
    }
}
impl PostgresBatchOptions {
    pub(super) fn validate(&self, connections: u32) -> QueueResult<()> {
        if self.workers == 0
            || self.workers >= connections
            || self.workers > 16
            || !(1..=128).contains(&self.max_jobs)
            || self.max_delay > Duration::from_millis(100)
        {
            return Err(QueueError::InvalidConfig("enqueue batching requires 1–16 workers below pool size, 1–128 jobs, and delay <=100 ms".into()));
        }
        Ok(())
    }
}
struct Request {
    prepared: PreparedEnqueue,
    reply: oneshot::Sender<QueueResult<JobId>>,
}
pub(super) struct Batcher {
    sender: mpsc::Sender<Request>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
impl Drop for Batcher {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
impl Batcher {
    pub(super) fn new(
        pool: bb8::Pool<Manager>,
        timeout: Duration,
        options: PostgresBatchOptions,
    ) -> Self {
        let (sender, receiver) =
            mpsc::channel::<Request>(options.max_jobs * options.workers as usize * 2);
        let receiver = Arc::new(Mutex::new(receiver));
        let tasks = (0..options.workers).map(|_| {
            let pool = pool.clone();
            let receiver = receiver.clone();
            let options = options.clone();
            tokio::spawn(async move {
                loop {
                    let mut batch = Vec::with_capacity(options.max_jobs);
                    {
                        let mut receiver = receiver.lock().await;
                        let Some(first) = receiver.recv().await else { return };
                        batch.push(first);
                        let deadline = tokio::time::Instant::now() + options.max_delay;
                        while batch.len() < options.max_jobs {
                            match receiver.try_recv() {
                                Ok(request) => batch.push(request),
                                Err(mpsc::error::TryRecvError::Disconnected) => break,
                                Err(mpsc::error::TryRecvError::Empty) => {
                                    match tokio::time::timeout_at(deadline, receiver.recv()).await {
                                        Ok(Some(request)) => batch.push(request),
                                        _ => break,
                                    }
                                }
                            }
                        }
                    }
                    batch.retain(|request| !request.reply.is_closed());
                    if batch.is_empty() { continue; }
                    // Consistent lock order prevents opposite-key batches deadlocking.
                    batch.sort_by(|a,b| a.prepared.lock_order().cmp(&b.prepared.lock_order()));
                    let result = tokio::time::timeout(timeout, async {
                        let mut client = pool.get().await.map_err(error)?;
                        let tx = client.transaction().await.map_err(error)?;
                        let ids = futures::future::try_join_all(batch.iter().map(|request|
                            enqueue_one(&tx, &request.prepared))).await?;
                        tx.commit().await.map_err(error)?;
                        Ok::<_, QueueError>(ids)
                    }).await.unwrap_or_else(|_| Err(error("PostgreSQL batch timed out; commit outcome may be unknown; retry with idempotency keys")));
                    match result {
                        Ok(ids) => for (request, id) in batch.into_iter().zip(ids) {
                            let _ = request.reply.send(Ok(id));
                        },
                        Err(error) => {
                            let message = error.to_string();
                            for request in batch {
                                let _ = request.reply.send(Err(QueueError::Internal(message.clone())));
                            }
                        }
                    }
                }
            })
        }).collect();
        Self { sender, tasks }
    }
    pub(super) async fn enqueue(&self, prepared: PreparedEnqueue) -> QueueResult<JobId> {
        let (reply, receiver) = oneshot::channel();
        self.sender
            .send(Request { prepared, reply })
            .await
            .map_err(|_| error("PostgreSQL batch worker unavailable"))?;
        receiver
            .await
            .map_err(|_| error("PostgreSQL batch worker stopped; commit outcome may be unknown"))?
    }
}
