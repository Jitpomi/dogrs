use crate::services::types::FleetParams;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogAfterHook, DogBeforeHook, HookContext};
use serde_json::Value;

pub struct BeforeRead;

#[async_trait]
impl DogBeforeHook<Value, FleetParams> for BeforeRead {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Validate delivery data before read
        Ok(())
    }
}

pub struct AfterRead;

#[async_trait]
impl DogAfterHook<Value, FleetParams> for AfterRead {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Post-process delivery data after read
        Ok(())
    }
}

pub struct BeforeWrite;

#[async_trait]
impl DogBeforeHook<Value, FleetParams> for BeforeWrite {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Validate delivery data before creation/update
        if let Some(delivery_data) = _ctx.data.as_ref().and_then(|v| v.as_object()) {
            // Validate required fields
            if !delivery_data.contains_key("pickup-address") {
                return Err(anyhow::anyhow!("Pickup address is required"));
            }
            if !delivery_data.contains_key("delivery-address") {
                return Err(anyhow::anyhow!("Delivery address is required"));
            }
        }
        Ok(())
    }
}

pub struct AfterWrite;

#[async_trait]
impl DogAfterHook<Value, FleetParams> for AfterWrite {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        Ok(())
    }
}
