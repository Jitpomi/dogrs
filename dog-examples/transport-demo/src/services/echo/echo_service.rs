use dog_core::{DogService, TenantContext};
use serde_json::Value;
pub struct Echo;
#[async_trait::async_trait]
impl DogService<Value, ()> for Echo {
    async fn create(&self, _: &TenantContext, data: Value, _: ()) -> anyhow::Result<Value> {
        Ok(data)
    }
}
