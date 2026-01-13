use crate::services::FleetParams;

pub fn configure(_app: &dog_core::DogApp<serde_json::Value, FleetParams>) -> anyhow::Result<()> {
    Ok(())
}
