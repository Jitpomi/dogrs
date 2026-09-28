use crate::services::types::AuthDemoParams;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogBeforeHook, HookContext};
use serde_json::Value;

pub fn author(params: &AuthDemoParams) -> Result<&str> {
    params
        .auth_result
        .as_ref()
        .and_then(|r| r.get("user"))
        .and_then(|u| u.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            dog_core::errors::DogError::not_authenticated("verified author required").into()
        })
}
pub struct ValidateMessageAuthorExists;
#[async_trait]
impl DogBeforeHook<Value, AuthDemoParams> for ValidateMessageAuthorExists {
    async fn run(&self, ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        let id = author(&ctx.params)?;
        if let Some(sender) = ctx.data.as_ref().and_then(|d| d.get("sender")) {
            anyhow::ensure!(
                sender.as_str() == Some(id),
                "sender must be the authenticated user"
            );
        }
        Ok(())
    }
}
