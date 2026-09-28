use crate::services::AuthDemoParams;
use dog_core::hooks::{DogBeforeHook, HookContext};
use serde_json::Value;
struct DemoTenant;
#[async_trait::async_trait]
impl DogBeforeHook<Value, AuthDemoParams> for DemoTenant {
    async fn run(&self, ctx: &mut HookContext<Value, AuthDemoParams>) -> anyhow::Result<()> {
        if ctx.tenant.tenant_id.0 != "default" {
            return Err(dog_core::errors::DogError::forbidden(
                "This auth demo has one fixed tenant: default",
            )
            .into());
        }
        Ok(())
    }
}
pub fn global_hooks(builder: &mut dog_core::DogAppBuilder<Value, AuthDemoParams>) {
    builder.hooks(|h| {
        h.before_all(std::sync::Arc::new(DemoTenant));
    });
}
