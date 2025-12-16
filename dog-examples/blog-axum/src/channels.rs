use crate::services::BlogParams;

pub fn configure(_app: &dog_core::DogApp<serde_json::Value, BlogParams>) -> anyhow::Result<()> {
    Ok(())
}
