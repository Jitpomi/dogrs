use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogBeforeHook, DogAfterHook, HookContext};
use serde_json::Value;
use crate::services::types::FleetParams;

pub struct BeforeRead;

#[async_trait]
impl DogBeforeHook<Value, FleetParams> for BeforeRead {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Validate vehicle query parameters
        if let Some(data) = &_ctx.data {
            if let Some(query_match) = data.get("match") {
                if let Some(match_str) = query_match.as_str() {
                    if !match_str.contains("vehicle") {
                        return Err(anyhow::anyhow!("Query must target vehicle entities"));
                    }
                }
            }
        }
        Ok(())
    }
}

pub struct AfterRead;

#[async_trait]
impl DogAfterHook<Value, FleetParams> for AfterRead {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Log vehicle queries for audit purposes
        println!("🚗 Vehicle query completed successfully");
        Ok(())
    }
}

pub struct BeforeWrite;

#[async_trait]
impl DogBeforeHook<Value, FleetParams> for BeforeWrite {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Validate vehicle data before writing
        if let Some(data) = &_ctx.data {
            // Ensure required fields are present
            if data.get("vehicle-id").is_none() {
                return Err(anyhow::anyhow!("vehicle-id is required"));
            }
            if data.get("license-plate").is_none() {
                return Err(anyhow::anyhow!("license-plate is required"));
            }
            if data.get("status").is_none() {
                return Err(anyhow::anyhow!("status is required"));
            }
        }
        Ok(())
    }
}

pub struct AfterWrite;

#[async_trait]
impl DogAfterHook<Value, FleetParams> for AfterWrite {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Log vehicle write operations for audit trail
        println!("🚗 Vehicle data write operation completed");
        Ok(())
    }
}
