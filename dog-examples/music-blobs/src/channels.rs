use crate::services::MusicParams;

pub fn configure(
    _app: &mut dog_core::DogAppBuilder<serde_json::Value, MusicParams>,
) -> anyhow::Result<()> {
    Ok(())
}
