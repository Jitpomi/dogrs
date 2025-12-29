use crate::services::MusicParams;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogBeforeHook, HookContext};
use serde_json::Value;

pub struct ProcessMulterParams;

#[async_trait]
impl DogBeforeHook<Value, MusicParams> for ProcessMulterParams {
    async fn run(&self, ctx: &mut HookContext<Value, MusicParams>) -> Result<()> {
        println!("🔧 Hook context:");
        println!("   Method: {:?}", ctx.method);
        println!("   Data: {:?}", ctx.data);
        println!("   Config: {:?}", ctx.config);

        Ok(())
    }
}
