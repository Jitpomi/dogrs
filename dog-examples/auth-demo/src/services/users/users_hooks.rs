use crate::services::types::AuthDemoParams;
use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogBeforeHook, HookContext};
use serde_json::Value;

/// Provider identity links can only be written by the trusted OAuth integration.
pub struct ProtectIdentityFields;
#[async_trait]
impl DogBeforeHook<Value, AuthDemoParams> for ProtectIdentityFields {
    async fn run(&self, ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        if ctx
            .params
            .provider
            .as_deref()
            .is_some_and(|p| !p.is_empty())
            && ctx
                .data
                .as_ref()
                .is_some_and(|d| d.get("googleId").is_some())
        {
            return Err(dog_core::errors::DogError::forbidden(
                "OAuth identity links are server-managed",
            )
            .into());
        }
        Ok(())
    }
}

pub fn authorize_user(params: &AuthDemoParams, id: &str) -> Result<()> {
    // Raw service lookups are used by JWT resolution before the hook has set its result.
    // External routes always authenticate via before hooks before calling this service.
    if params.authenticated {
        let own = params
            .auth_result
            .as_ref()
            .and_then(|r| r.get("user"))
            .and_then(|u| u.get("id"))
            .and_then(Value::as_str);
        if own != Some(id) {
            return Err(dog_core::errors::DogError::forbidden(
                "Only your own user record is accessible",
            )
            .into());
        }
    }
    Ok(())
}
