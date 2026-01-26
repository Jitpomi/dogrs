use anyhow::Result;
use dog_axum::{axum, AxumApp};
use dog_core::DogApp;
use serde_json::Value;
use crate::services::SocialParams;

pub async fn social_app() -> Result<AxumApp<Value, SocialParams>> {
    let dog_app: DogApp<Value, SocialParams> = DogApp::new();
    dog_app.set("http.host", "127.0.0.1");
    dog_app.set("http.port", "3036");
    crate::hooks::global_hooks(&dog_app);
    crate::channels::configure(&dog_app)?;
    let social_app: AxumApp<Value, SocialParams> = axum(dog_app);
    crate::typedb::TypeDBState::setup_db(social_app.app.as_ref()).await?;
    Ok(social_app)
}
