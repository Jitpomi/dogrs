use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogBeforeHook, DogAfterHook, HookContext};
use serde_json::Value;
use crate::services::types::FleetParams;

pub struct BeforeRead;

#[async_trait]
impl DogBeforeHook<Value, FleetParams> for BeforeRead {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Validate employee query parameters
        if let Some(data) = &_ctx.data {
            if let Some(query_match) = data.get("match") {
                if let Some(match_str) = query_match.as_str() {
                    if !match_str.contains("employee") {
                        return Err(anyhow::anyhow!("Query must target employee entities"));
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
        // Log employee queries for audit purposes
        println!("👥 Employee query completed successfully");
        Ok(())
    }
}

pub struct BeforeWrite;

#[async_trait]
impl DogBeforeHook<Value, FleetParams> for BeforeWrite {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Validate employee data before writing
        if let Some(data) = &_ctx.data {
            // Ensure required fields are present
            if data.get("employee-id").is_none() {
                return Err(anyhow::anyhow!("employee-id is required"));
            }
            if data.get("role").is_none() {
                return Err(anyhow::anyhow!("role is required"));
            }
        }
        Ok(())
    }
}

pub struct AfterWrite;

#[async_trait]
impl DogAfterHook<Value, FleetParams> for AfterWrite {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Log employee write operations for audit trail
        println!("👥 Employee data write operation completed");
        Ok(())
    }
}
