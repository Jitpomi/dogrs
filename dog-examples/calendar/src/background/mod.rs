//! # Background Queue & Scheduled Reminders (`background/mod.rs`)
//!
//! Demonstrates `dog-queue` integration for delayed job execution:
//! - Defines [`EventReminderJob`] which executes at `start_time - 15 minutes`.
//! - Uses [`dog_queue::backend::memory::MemoryBackend`] for zero-dependency in-memory delayed scheduling.
//! - Runs workers that broadcast real-time `"reminder"` notifications over [`CalendarJobContext::event_channel`].
//! - Supports cancel-wins guarantees: when an event is deleted or rescheduled, pending reminder jobs are cancelled.

use std::sync::Arc;

use async_trait::async_trait;
use dog_queue::backend::memory::MemoryBackend;
use dog_queue::{Job, JobError, JobPriority, QueueAdapter, QueueConfig, QueueCtx, WorkerHandle};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex};

/// Context passed to background job execution, providing access to the real-time event channel.
#[derive(Clone)]
pub struct CalendarJobContext {
    pub event_channel: Arc<broadcast::Sender<serde_json::Value>>,
}

/// Delayed reminder job scheduled by dog-queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventReminderJob {
    pub tenant: String,
    pub event_id: String,
    pub title: String,
    pub start_time: String,
}

#[async_trait]
impl Job for EventReminderJob {
    type Context = CalendarJobContext;
    type Result = ();

    const JOB_TYPE: &'static str = "event-reminder";
    const PRIORITY: JobPriority = JobPriority::High;
    const MAX_RETRIES: u32 = 2;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let reminder_event = serde_json::json!({
            "tenant": self.tenant,
            "event": "reminder",
            "data": {
                "tenant": self.tenant,
                "event_id": self.event_id,
                "title": self.title,
                "start_time": self.start_time,
                "message": format!("Upcoming Event Reminder: {}", self.title)
            }
        });

        // Broadcast to all connected WebSocket clients
        let _ = ctx.event_channel.send(reminder_event);
        Ok(())
    }
}

/// Background queue management using dog-queue in-memory backend.
pub struct CalendarQueue {
    pub adapter: Arc<QueueAdapter<MemoryBackend>>,
    pub worker_handle: Mutex<Option<WorkerHandle>>,
}

impl CalendarQueue {
    /// Creates a new in-memory queue adapter and registers the reminder job.
    pub async fn new() -> anyhow::Result<Self> {
        let backend = MemoryBackend::new();
        // Keep workers active indefinitely for long-running service execution
        let config = QueueConfig {
            worker_idle_timeout: std::time::Duration::from_secs(86400 * 365),
            ..Default::default()
        };
        let adapter = Arc::new(QueueAdapter::with_config(backend, config));
        adapter.register_job::<EventReminderJob>().await?;

        Ok(Self {
            adapter,
            worker_handle: Mutex::new(None),
        })
    }

    /// Spawns background workers to poll and execute delayed reminder jobs.
    pub async fn start_workers(&self, context: CalendarJobContext) -> anyhow::Result<()> {
        let ctx = QueueCtx::new("default".to_string());
        let handle = self
            .adapter
            .start_workers(ctx, context, vec![EventReminderJob::JOB_TYPE.to_string()])
            .await?;

        *self.worker_handle.lock().await = Some(handle);
        Ok(())
    }
}
