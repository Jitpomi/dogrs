use crate::services::SocialParams;

pub fn configure(_app: &dog_core::DogApp<serde_json::Value, SocialParams>) -> anyhow::Result<()> {
    Ok(())
}
