use crate::services::MusicParams;

pub fn configure(_app: &dog_core::DogApp<serde_json::Value, MusicParams>) -> anyhow::Result<()> {
    Ok(())
}
