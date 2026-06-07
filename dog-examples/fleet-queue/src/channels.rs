use crate::services::FleetParams;

pub fn configure(
    _app: &mut dog_core::DogAppBuilder<serde_json::Value, FleetParams>,
) -> anyhow::Result<()> {
    Ok(())
}
