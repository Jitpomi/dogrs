//! # Calendar In-Memory Adapter (`services/adapters/calendar_adapter.rs`)
//!
//! Provides the pure data access layer for calendar events:
//! - Multi-tenant in-memory storage partitioned by `TenantContext`.
//! - Standard CRUD operations (`_create`, `_find`, `_get`, `_update`, `_patch`, `_remove`).

use std::sync::Arc;

use anyhow::Result;
use dog_core::errors::DogError;
use dog_core::tenant::TenantContext;
use dog_core::ServiceCapabilities;
use serde_json::Value;
use uuid::Uuid;

use crate::services::{CalendarParams, CalendarState};

/// Multi-tenant in-memory adapter for event scheduling and storage.
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

    /// Creates a new calendar event.
    pub async fn _create(
        &self,
        ctx: &TenantContext,
        data: Value,
        _params: CalendarParams,
    ) -> Result<Value> {
        let mut obj = data.as_object().cloned().unwrap_or_default();

        // Always issue a fresh server-authoritative UUID to prevent ID squatting or collisions
        let id = format!("{}:{}", self.id_prefix, Uuid::new_v4());
        obj.insert("id".to_string(), Value::String(id.clone()));
        let value = Value::Object(obj);

        let tenant = Self::tenant_key(ctx);
        let mut by_tenant = self.state.events_by_tenant.write().await;
        by_tenant
            .entry(tenant)
            .or_default()
            .insert(id, value.clone());

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

    /// Partially updates an existing event.
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
        let map = by_tenant.entry(tenant).or_default();

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

        Ok(value)
    }

    /// Removes an event from storage.
    pub async fn _remove(
        &self,
        ctx: &TenantContext,
        id: Option<&str>,
        _params: CalendarParams,
    ) -> Result<Value> {
        let id = self.require_id(id, "Remove requires an id")?;
        let tenant = Self::tenant_key(ctx);

        let mut by_tenant = self.state.events_by_tenant.write().await;
        let map = by_tenant.entry(tenant).or_default();
        let removed = map.remove(id).ok_or_else(|| self.not_found(id))?;

        Ok(removed)
    }
}

dog_core::dog_adapter!(
    CalendarAdapter,
    serde_json::Value,
    crate::services::CalendarParams
);
