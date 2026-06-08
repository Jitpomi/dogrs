use crate::services::FleetParams;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogAfterHook, DogBeforeHook, HookContext};
use serde_json::Value;

pub struct BeforeEnqueue;

#[async_trait]
impl DogBeforeHook<Value, FleetParams> for BeforeEnqueue {
    async fn run(&self, ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        if let Some(data) = &ctx.data {
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
        Ok(())
    }
}
