use crate::services::AuthDemoParams;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogAfterHook, HookContext, HookResult};
use serde_json::Value;
pub struct StripPasswordFromAuthResult;

#[async_trait]
impl DogAfterHook<Value, AuthDemoParams> for StripPasswordFromAuthResult {
    async fn run(&self, ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        let Some(result) = ctx.result.as_mut() else {
            return Ok(());
        };

        match result {
            HookResult::One(v) => {
                if let Some(user) = v.get_mut("user").and_then(|u| u.as_object_mut()) {
                    user.remove("password");
                }
            }
            HookResult::Many(items) => {
                for v in items {
                    if let Some(user) = v.get_mut("user").and_then(|u| u.as_object_mut()) {
                        user.remove("password");
                    }
                }
            }
        }

        Ok(())
    }
}
