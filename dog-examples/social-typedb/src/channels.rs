use crate::services::SocialParams;

pub fn configure(
    _app: &mut dog_core::DogAppBuilder<serde_json::Value, SocialParams>,
) -> anyhow::Result<()> {
    Ok(())
}
