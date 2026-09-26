// src/scheduling/mod.rs

//! Scheduling support for recurring jobs (cron‑based).
//! This module is compiled only when the `cron-scheduling` feature is enabled.
//! It provides a lightweight in‑process scheduler that repeatedly enqueues
//! a pre‑encoded job according to a cron expression. The design is deliberately
//! simple and platform‑agnostic – back‑ends can implement their own persistent
//! scheduler and plug it in via the `SchedulerBackend` trait.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
pub use cron::Schedule;
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{sleep_until, Instant};
use uuid::Uuid;

use crate::{backend::QueueBackend, QueueAdapter, QueueCtx, QueueError, JobMessage};

/// A handle that uniquely identifies a scheduled recurring job.
/// Dropping the handle does **not** cancel the job – call `cancel` on the
/// `Scheduler` to stop it.
#[derive(Debug)]
pub struct ScheduleHandle {
    pub id: Uuid,
    cancel_tx: oneshot::Sender<()>,
}

impl ScheduleHandle {
    /// Cancel the associated recurring job.
    pub fn cancel(self) {
        let _ = self.cancel_tx.send(());
    }
}

/// Trait for pluggable scheduler back‑ends. The in‑process implementation
/// lives in this crate; external services (e.g. Redis, Postgres `pg_cron`,
/// Cloud Scheduler) can provide their own implementations and expose the same
/// API.
pub trait SchedulerBackend: Send + Sync {
    /// Schedule a job according to `cron_expr`.
    fn schedule(
        &self,
        cron_expr: String,
        msg: JobMessage,
        ctx: QueueCtx,
    ) -> Result<ScheduleHandle, QueueError>;

    /// Cancel a previously scheduled job.
    fn cancel(&self, handle: ScheduleHandle) -> Result<(), QueueError>;
}

/// Simple in‑process scheduler that keeps a map of spawn‑ed tokio tasks.
/// It is **not** durable across restarts – for persistence enable a backend
/// implementation (e.g. RedisScheduler) and plug it in via the trait.
pub struct InMemoryScheduler<B: QueueBackend + ?Sized> {
    adapter: QueueAdapter<B>,
    tasks: Arc<Mutex<HashMap<Uuid, JoinHandle<()>>>>,
}

impl<B: QueueBackend + ?Sized + 'static> InMemoryScheduler<B> {
    /// Construct a new scheduler for the given adapter.
    pub fn new(adapter: QueueAdapter<B>) -> Self {
        Self {
            adapter,
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<B: QueueBackend + ?Sized + 'static> SchedulerBackend for InMemoryScheduler<B> {
    fn schedule(
        &self,
        cron_expr: String,
        msg: JobMessage,
        ctx: QueueCtx,
    ) -> Result<ScheduleHandle, QueueError> {
        let schedule = Schedule::from_str(&cron_expr)
            .map_err(|e| QueueError::InvalidConfig(format!("Invalid cron expression: {}", e)))?;
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        let id = Uuid::new_v4();
        let adapter = self.adapter.clone();
        let tasks = self.tasks.clone();

        // Spawn a background task that waits for each upcoming datetime and enqueues.
        let join_handle = tokio::spawn(async move {
            let mut upcoming = schedule.upcoming(Utc);
            loop {
                let next = match upcoming.next() {
                    Some(t) => t,
                    None => break,
                };
                let now = Utc::now();
                if next > now {
                    let dur = (next - now).to_std().unwrap_or(Duration::from_secs(0));
                    let deadline = Instant::now() + dur;
                    tokio::select! {
                        _ = sleep_until(deadline) => {}
                        _ = &mut cancel_rx => {
                            break;
                        }
                    }
                }
                // Enqueue the job – ignore errors after logging.
                if let Err(e) = adapter
                    .enqueue_message(ctx.clone(), msg.clone())
                    .await
                {
                    tracing::error!("Failed to enqueue recurring job: {}", e);
                }
            }
        });

        // Store the handle so we can abort on cancel.
        let rt = tokio::runtime::Handle::current();
        rt.spawn(async move {
            let mut lock = tasks.lock().await;
            lock.insert(id, join_handle);
        });

        Ok(ScheduleHandle { id, cancel_tx })
    }

    fn cancel(&self, handle: ScheduleHandle) -> Result<(), QueueError> {
        let mut lock = futures::executor::block_on(self.tasks.lock());
        if let Some(task) = lock.remove(&handle.id) {
            task.abort();
        }
        // Sending the cancellation signal ensures the task loop breaks if it is sleeping.
        let _ = handle.cancel_tx.send(());
        Ok(())
    }
}

/// Public façade that users interact with. It simply forwards to the loaded
/// backend implementation. Currently we expose the in‑memory scheduler as the
/// default; external back‑ends can be added behind a feature flag.
#[derive(Clone)]
pub struct Scheduler<B: QueueBackend + ?Sized> {
    inner: Arc<dyn SchedulerBackend>,
    _phantom: std::marker::PhantomData<B>,
}

impl<B: QueueBackend + ?Sized + 'static> Scheduler<B> {
    /// Construct a scheduler using the in‑memory backend.
    pub fn new(adapter: QueueAdapter<B>) -> Self {
        let backend = InMemoryScheduler::new(adapter);
        Self {
            inner: Arc::new(backend),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Schedule a recurring job.
    pub fn schedule(
        &self,
        cron_expr: String,
        msg: JobMessage,
        ctx: QueueCtx,
    ) -> Result<ScheduleHandle, QueueError> {
        self.inner.schedule(cron_expr, msg, ctx)
    }

    /// Cancel a recurring schedule.
    pub fn cancel(&self, handle: ScheduleHandle) -> Result<(), QueueError> {
        self.inner.cancel(handle)
    }
}
