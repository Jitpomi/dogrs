use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogBeforeHook, DogAfterHook, HookContext};
use serde_json::Value;
use crate::services::FleetParams;

pub struct BeforeEnqueue;

#[async_trait]
impl DogBeforeHook<Value, FleetParams> for BeforeEnqueue {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Validate job data before enqueueing
        if let Some(data) = &_ctx.data {
            if data.get("job_type").is_none() {
                return Err(anyhow::anyhow!("job_type is required"));
            }
        }
        Ok(())
    }
}

pub struct AfterEnqueue;

#[async_trait]
impl DogAfterHook<Value, FleetParams> for AfterEnqueue {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Log successful job enqueue
        if let Some(_result) = &_ctx.result {
            println!("📋 Job enqueued successfully");
        }
        Ok(())
    }
}

pub struct BeforeStats;

#[async_trait]
impl DogBeforeHook<Value, FleetParams> for BeforeStats {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Pre-process stats request if needed
        Ok(())
    }
}

pub struct AfterStats;

#[async_trait]
impl DogAfterHook<Value, FleetParams> for AfterStats {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Post-process stats response if needed
        Ok(())
    }
}
