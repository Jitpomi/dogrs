use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogBeforeHook, HookContext};
use serde_json::Value;
use crate::services::types::AuthDemoParams;

pub struct EnforceUserOnDelete;

#[async_trait]
impl DogBeforeHook<Value, AuthDemoParams> for EnforceUserOnDelete {
    async fn run(&self, _ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        // Enforce user deletion policies
        Ok(())
    }
}
