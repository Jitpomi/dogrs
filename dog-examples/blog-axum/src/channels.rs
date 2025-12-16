use crate::services::RelayParams;

pub fn configure(_app: &dog_core::DogApp<serde_json::Value, RelayParams>) -> anyhow::Result<()> {
    Ok(())
}
