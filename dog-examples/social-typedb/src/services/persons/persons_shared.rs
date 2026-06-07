use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use dog_core::{DogApp, DogBeforeHook, HookContext, ServiceCapabilities, ServiceMethodKind};
use dog_schema::Rules;

use crate::services::SocialParams;

pub fn capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Custom("read"),
        ServiceMethodKind::Custom("write"),
    ])
}

/// Validates incoming write data for the persons service.
///
/// Uses [`dog_schema::Rules`] for structured, aggregated error reporting.
/// Registered on `before_all` because the service uses
/// `ServiceMethodKind::Custom("write")` — not the standard Create/Patch/Update
/// methods that [`dog_schema::WriteMethods`] filters on.
struct PersonsWriteValidator;

#[async_trait]
impl DogBeforeHook<Value, SocialParams> for PersonsWriteValidator {
    async fn run(&self, ctx: &mut HookContext<Value, SocialParams>) -> Result<()> {
        // Only validate on TypeDB "write" custom operations.
        if ctx.method != ServiceMethodKind::Custom("write") {
            return Ok(());
        }

        let Some(data) = ctx.data.as_ref() else {
            return Ok(());
        };

        // Use dog_schema::Rules to accumulate and report field errors together.
        let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("");
        Rules::new()
            .non_empty("name", name)
            .min_len("name", name, 2)
            .max_len("name", name, 100)
            .check()
    }
}

pub fn register_hooks(app: &DogApp<Value, SocialParams>) -> Result<()> {
    app.hooks(|h| {
        h.before_all(Arc::new(PersonsWriteValidator));
    });
    Ok(())
}
