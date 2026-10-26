//! # Calendar In-Memory Adapter (`services/adapters/calendar_adapter.rs`)
//!
//! Provides the data access layer for calendar events:
//! - Multi-tenant in-memory storage partitioned by `TenantContext`.
//! - Standard CRUD operations (`_create`, `_find`, `_get`, `_update`, `_patch`, `_remove`).
//! - Automatic background reminder scheduling via `dog-queue`.
//! - **Cancel-wins semantics**: When an event is removed or rescheduled, pending reminder jobs
//!   are automatically canceled to prevent phantom notifications.

use std::sync::Arc;

use anyhow::Result;
use dog_core::errors::DogError;
use dog_core::tenant::TenantContext;
use dog_core::ServiceCapabilities;
use dog_queue::{EnqueueOptions, QueueCtx};
use serde_json::Value;
use uuid::Uuid;

use crate::background::EventReminderJob;
use crate::services::{CalendarParams, CalendarState};

/// Multi-tenant in-memory adapter for event scheduling and reminders.
pub struct CalendarAdapter {
    pub state: Arc<CalendarState>,
    pub id_prefix: &'static str,
    pub not_found_prefix: &'static str,
    pub capabilities: ServiceCapabilities,
}

impl CalendarAdapter {
    fn tenant_key(ctx: &TenantContext) -> String {
        ctx.tenant_id.0.clone()
    }

    fn not_found(&self, id: &str) -> anyhow::Error {
        DogError::not_found(format!("{}: {id}", self.not_found_prefix)).into_anyhow()
    }

    fn require_id<'a>(&self, id: Option<&'a str>, msg: &'static str) -> Result<&'a str> {
        id.ok_or_else(|| DogError::bad_request(msg).into_anyhow())
    }

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

    /// Creates a new calendar event and schedules a delayed reminder via `dog-queue`.
    pub async fn _create(
        &self,
        ctx: &TenantContext,
        data: Value,
        _params: CalendarParams,
    ) -> Result<Value> {
        let mut obj = data.as_object().cloned().unwrap_or_default();

        let id = obj
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{}:{}", self.id_prefix, Uuid::new_v4()));

        obj.insert("id".to_string(), Value::String(id.clone()));
        let value = Value::Object(obj);

        let tenant = Self::tenant_key(ctx);
        let mut by_tenant = self.state.events_by_tenant.write().await;
        by_tenant
            .entry(tenant.clone())
            .or_default()
            .insert(id.clone(), value.clone());

        // Schedule delayed reminder via dog-queue if start_time is provided
        if let Some(start_utc) = Self::parse_start_time(value.get("start_time")) {
            let title = value
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Untitled Event")
                .to_string();
            let start_time_str = value
                .get("start_time")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            // Reminder 15 minutes before start (or immediately if start is sooner)
            let reminder_time = (start_utc - chrono::Duration::minutes(15)).max(chrono::Utc::now());

            let job = EventReminderJob {
                event_id: id.clone(),
                title,
                start_time: start_time_str,
            };

            let queue_ctx = QueueCtx::new(tenant);
            let opts = EnqueueOptions::scheduled(reminder_time);
            if let Ok(job_id) = self
                .state
                .queue
                .adapter
                .enqueue_opts(queue_ctx, job, opts)
                .await
            {
                self.state.reminder_jobs.write().await.insert(id, job_id);
            }
        }

        Ok(value)
    }

    /// Finds all events belonging to the caller's tenant.
    pub async fn _find(&self, ctx: &TenantContext, _params: CalendarParams) -> Result<Vec<Value>> {
        let tenant = Self::tenant_key(ctx);
        let by_tenant = self.state.events_by_tenant.read().await;
        let map = by_tenant.get(&tenant);
        Ok(map.into_iter().flat_map(|m| m.values()).cloned().collect())
    }

    /// Retrieves a single event by ID within the caller's tenant boundary.
    pub async fn _get(
        &self,
        ctx: &TenantContext,
        id: &str,
        _params: CalendarParams,
    ) -> Result<Value> {
        let tenant = Self::tenant_key(ctx);
        let by_tenant = self.state.events_by_tenant.read().await;
        let map = by_tenant.get(&tenant);
        map.and_then(|m| m.get(id))
            .cloned()
            .ok_or_else(|| self.not_found(id))
    }

    /// Replaces an existing event record.
    pub async fn _update(
        &self,
        ctx: &TenantContext,
        id: &str,
        data: Value,
        params: CalendarParams,
    ) -> Result<Value> {
        self._patch(ctx, Some(id), data, params).await
    }

    /// Partially updates an existing event and updates its scheduled reminder if rescheduled.
    pub async fn _patch(
        &self,
        ctx: &TenantContext,
        id: Option<&str>,
        data: Value,
        _params: CalendarParams,
    ) -> Result<Value> {
        let id = self.require_id(id, "Patch requires an id")?;

        let tenant = Self::tenant_key(ctx);
        let mut by_tenant = self.state.events_by_tenant.write().await;
        let map = by_tenant.entry(tenant.clone()).or_default();

        let existing = map.get(id).cloned().ok_or_else(|| self.not_found(id))?;

        let mut record = existing.as_object().cloned().unwrap_or_default();
        if let Some(patch) = data.as_object() {
            for (k, v) in patch {
                if k == "id" {
                    continue;
                }
                record.insert(k.clone(), v.clone());
            }
        }

        record.insert("id".to_string(), Value::String(id.to_string()));
        let value = Value::Object(record);
        map.insert(id.to_string(), value.clone());

        // Reschedule reminder if start_time was updated
        if data.get("start_time").is_some() {
            // Cancel-wins: cancel existing reminder
            if let Some(old_job_id) = self.state.reminder_jobs.write().await.remove(id) {
                let queue_ctx = QueueCtx::new(tenant.clone());
                let _ = self.state.queue.adapter.cancel(queue_ctx, old_job_id).await;
            }

            // Schedule updated reminder
            if let Some(start_utc) = Self::parse_start_time(value.get("start_time")) {
                let title = value
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled Event")
                    .to_string();
                let start_time_str = value
                    .get("start_time")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();

                let reminder_time =
                    (start_utc - chrono::Duration::minutes(15)).max(chrono::Utc::now());
                let job = EventReminderJob {
                    event_id: id.to_string(),
                    title,
                    start_time: start_time_str,
                };

                let queue_ctx = QueueCtx::new(tenant);
                let opts = EnqueueOptions::scheduled(reminder_time);
                if let Ok(new_job_id) = self
                    .state
                    .queue
                    .adapter
                    .enqueue_opts(queue_ctx, job, opts)
                    .await
                {
                    self.state
                        .reminder_jobs
                        .write()
                        .await
                        .insert(id.to_string(), new_job_id);
                }
            }
        }

        Ok(value)
    }

    /// Removes an event and enforces cancel-wins on any pending reminder jobs.
    pub async fn _remove(
        &self,
        ctx: &TenantContext,
        id: Option<&str>,
        _params: CalendarParams,
    ) -> Result<Value> {
        let id = self.require_id(id, "Remove requires an id")?;

        let tenant = Self::tenant_key(ctx);
        let mut by_tenant = self.state.events_by_tenant.write().await;
        let map = by_tenant.entry(tenant.clone()).or_default();
        let removed = map.remove(id).ok_or_else(|| self.not_found(id))?;

        // Cancel-wins: cancel pending reminder job when event is removed
        if let Some(job_id) = self.state.reminder_jobs.write().await.remove(id) {
            let queue_ctx = QueueCtx::new(tenant);
            let _ = self.state.queue.adapter.cancel(queue_ctx, job_id).await;
        }

        Ok(removed)
    }
}

dog_core::dog_adapter!(
    CalendarAdapter,
    serde_json::Value,
    crate::services::CalendarParams
);
