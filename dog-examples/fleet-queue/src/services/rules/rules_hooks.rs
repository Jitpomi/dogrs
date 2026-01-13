use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogBeforeHook, DogAfterHook, HookContext};
use serde_json::Value;
use crate::services::types::FleetParams;

pub struct BeforeRead;

#[async_trait]
impl DogBeforeHook<Value, FleetParams> for BeforeRead {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Validate rules query parameters
        if let Some(data) = &_ctx.data {
            if let Some(query_match) = data.get("match") {
                if let Some(match_str) = query_match.as_str() {
                    if !match_str.contains("rule") {
                        return Err(anyhow::anyhow!("Query must target rule entities"));
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
        // Log rules queries for audit purposes
        println!("📋 Rules query completed successfully");
        Ok(())
    }
}

pub struct BeforeWrite;

#[async_trait]
impl DogBeforeHook<Value, FleetParams> for BeforeWrite {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Validate rules data before writing
        if let Some(data) = &_ctx.data {
            // Ensure required fields are present
            if data.get("rule-name").is_none() {
                return Err(anyhow::anyhow!("rule-name is required"));
            }
            if data.get("rule-value").is_none() {
                return Err(anyhow::anyhow!("rule-value is required"));
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
        // Log rules write operations for audit trail
        println!("📋 Rules data write operation completed");
        Ok(())
    }
}
