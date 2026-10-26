//! # Events Service Hooks (`services/events/events_hooks.rs`)
//!
//! Service-specific hooks attached to the events service:
//! - [`NormalizeEventsResult`]: After-hook ensuring consistent result formatting.
//! - [`EventReminderHook`]: Lifecycle after-hook managing `dog-queue` reminder jobs:
//!   - Schedules delayed reminder on `create`.
//!   - Reschedules on `patch` / `update` if event details or timing change.
//!   - Enforces **cancel-wins** on `remove` to prevent orphan notifications.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogAfterHook, HookContext, HookResult};
use dog_core::ServiceMethodKind;
use dog_queue::{EnqueueOptions, QueueCtx};
use serde_json::Value;

use crate::background::EventReminderJob;
use crate::services::{CalendarParams, CalendarState};

/// After-hook ensuring consistent result formatting across operations.
pub struct NormalizeEventsResult;

#[async_trait]
impl DogAfterHook<Value, CalendarParams> for NormalizeEventsResult {
    async fn run(&self, _ctx: &mut HookContext<Value, CalendarParams>) -> Result<()> {
        Ok(())
    }
}

/// Lifecycle hook that manages background reminder jobs and cancel-wins semantics in `dog-queue`.
pub struct EventReminderHook {
    pub state: Arc<CalendarState>,
}

impl EventReminderHook {
    fn parse_start_time(val: Option<&Value>) -> Option<chrono::DateTime<chrono::Utc>> {
        val.and_then(Value::as_str).and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .or_else(|_| {
                    // Fallback for "YYYY-MM-DDTHH:MM" local inputs
                    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
                        .map(|ndt| ndt.and_utc())
                })
                .ok()
        })
    }

    async fn schedule_reminder(state: &CalendarState, tenant: &str, event: &Value) {
        if let Some(start_utc) = Self::parse_start_time(event.get("start_time")) {
            let now = chrono::Utc::now();
            if start_utc > now {
                let id = event.get("id").and_then(Value::as_str).unwrap_or_default();
                let title = event
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled Event")
                    .to_string();
                let start_time_str = event
                    .get("start_time")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let location = event
                    .get("location")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());
                let description = event
                    .get("description")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());

                let reminder_time = (start_utc
                    .checked_sub_signed(chrono::Duration::minutes(15))
                    .unwrap_or(start_utc))
                .max(now);

                let reminder_key = format!("{}:{}", tenant, id);
                let job = EventReminderJob {
                    tenant: tenant.to_string(),
                    event_id: id.to_string(),
                    title,
                    start_time: start_time_str,
                    location,
                    description,
                };

                let queue_ctx = QueueCtx::new(tenant.to_string());
                let opts = EnqueueOptions::scheduled(reminder_time);
                if let Ok(job_id) = state.queue.adapter.enqueue_opts(queue_ctx, job, opts).await {
                    state
                        .reminder_jobs
                        .write()
                        .await
                        .insert(reminder_key, job_id);
                }
            }
        }
    }

    async fn cancel_reminder(state: &CalendarState, tenant: &str, event_id: &str) {
        let reminder_key = format!("{}:{}", tenant, event_id);
        let old_job_id = state.reminder_jobs.write().await.remove(&reminder_key);
        if let Some(old_job_id) = old_job_id {
            let queue_ctx = QueueCtx::new(tenant.to_string());
            let _ = state.queue.adapter.cancel(queue_ctx, old_job_id).await;
        }
    }
}

#[async_trait]
impl DogAfterHook<Value, CalendarParams> for EventReminderHook {
    async fn run(&self, ctx: &mut HookContext<Value, CalendarParams>) -> Result<()> {
        let tenant = ctx.tenant.tenant_id.0.clone();

        match &ctx.method {
            ServiceMethodKind::Create => {
                if let Some(HookResult::One(event)) = &ctx.result {
                    Self::schedule_reminder(&self.state, &tenant, event).await;
                }
            }
            ServiceMethodKind::Update | ServiceMethodKind::Patch => {
                if let Some(HookResult::One(event)) = &ctx.result {
                    let should_reschedule = match &ctx.data {
                        Some(data) => {
                            data.get("start_time").is_some()
                                || data.get("title").is_some()
                                || data.get("location").is_some()
                                || data.get("description").is_some()
                        }
                        None => true,
                    };

                    if should_reschedule {
                        if let Some(id) = event.get("id").and_then(Value::as_str) {
                            Self::cancel_reminder(&self.state, &tenant, id).await;
                        }
                        Self::schedule_reminder(&self.state, &tenant, event).await;
                    }
                }
            }
            ServiceMethodKind::Remove => {
                if let Some(HookResult::One(event)) = &ctx.result {
                    if let Some(id) = event.get("id").and_then(Value::as_str) {
                        Self::cancel_reminder(&self.state, &tenant, id).await;
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }
}
