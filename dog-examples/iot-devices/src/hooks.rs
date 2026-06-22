use crate::services::DemoParams;
use serde_json::Value;

pub fn register_global_hooks(
    _app: &mut dog_core::DogAppBuilder<Value, DemoParams>,
) -> anyhow::Result<()> {
    Ok(())
}
