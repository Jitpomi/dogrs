use crate::services::types::AuthDemoParams;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogAfterHook, DogBeforeHook, HookContext};
use serde_json::Value;

pub struct ValidateMessageAuthorExists;

#[async_trait]
impl DogBeforeHook<Value, AuthDemoParams> for ValidateMessageAuthorExists {
    async fn run(&self, _ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        // Validate that message author exists
        Ok(())
    }
}

pub struct ExpandMessageAuthor;

#[async_trait]
impl DogAfterHook<Value, AuthDemoParams> for ExpandMessageAuthor {
    async fn run(&self, _ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        // Expand message author information
        Ok(())
    }
}

pub struct NormalizeMessagesResult;

#[async_trait]
impl DogAfterHook<Value, AuthDemoParams> for NormalizeMessagesResult {
    async fn run(&self, _ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        // Normalize messages result format
        Ok(())
    }
}
