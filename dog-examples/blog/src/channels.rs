use crate::services::BlogParams;

pub fn configure(
    _app: &mut dog_core::DogAppBuilder<serde_json::Value, BlogParams>,
) -> anyhow::Result<()> {
    Ok(())
}
