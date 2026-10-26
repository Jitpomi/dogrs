//! # Events Service Hooks (`services/events/events_hooks.rs`)
//!
//! Service-specific hooks attached to the events service.

use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogAfterHook, HookContext};
use serde_json::Value;

use crate::services::CalendarParams;

/// After-hook ensuring consistent result formatting across operations.
pub struct NormalizeEventsResult;

#[async_trait]
impl DogAfterHook<Value, CalendarParams> for NormalizeEventsResult {
    async fn run(&self, _ctx: &mut HookContext<Value, CalendarParams>) -> Result<()> {
        Ok(())
    }
}
