//! # Events Service Implementation (`services/events/events_service.rs`)
//!
//! Implements the [`dog_core::DogService`] contract for calendar scheduling:
//! - Implements all standard CRUD operations: `create`, `find`, `get`, `update`, `patch`, `remove`.
//! - Delegates storage persistence and reminder queueing to [`CalendarAdapter`].

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use serde_json::Value;

use crate::services::adapters::calendar_adapter::CalendarAdapter;
use crate::services::{CalendarParams, CalendarState};

use super::events_shared;

/// Calendar events domain service.
pub struct EventsService {
    pub adapter: CalendarAdapter,
}

#[async_trait]
impl DogService<Value, CalendarParams> for EventsService {
    fn capabilities(&self) -> ServiceCapabilities {
        events_shared::crud_capabilities()
    }

    async fn create(
        &self,
        ctx: &TenantContext,
        data: Value,
        params: CalendarParams,
    ) -> Result<Value> {
        self.adapter._create(ctx, data, params).await
    }

    async fn find(&self, ctx: &TenantContext, params: CalendarParams) -> Result<Vec<Value>> {
        self.adapter._find(ctx, params).await
    }

    async fn get(&self, ctx: &TenantContext, id: &str, params: CalendarParams) -> Result<Value> {
        self.adapter._get(ctx, id, params).await
    }

    async fn update(
        &self,
        ctx: &TenantContext,
        id: &str,
        data: Value,
        params: CalendarParams,
    ) -> Result<Value> {
        self.adapter._update(ctx, id, data, params).await
    }

    async fn patch(
        &self,
        ctx: &TenantContext,
        id: Option<&str>,
        data: Value,
        params: CalendarParams,
    ) -> Result<Value> {
        self.adapter._patch(ctx, id, data, params).await
    }

    async fn remove(
        &self,
        ctx: &TenantContext,
        id: Option<&str>,
        params: CalendarParams,
    ) -> Result<Value> {
        self.adapter._remove(ctx, id, params).await
    }
}

impl EventsService {
    /// Constructs a new EventsService backed by the given application state.
    pub fn new(state: Arc<CalendarState>) -> Self {
        Self {
            adapter: CalendarAdapter {
                state,
                id_prefix: "event",
                not_found_prefix: "Event not found",
                capabilities: events_shared::crud_capabilities(),
            },
        }
    }
}
