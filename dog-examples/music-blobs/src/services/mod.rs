use std::sync::Arc;

use crate::rustfs::RustFsState;
use dog_core::DogService;

pub mod types;
pub use types::MusicParams;

pub mod adapters;
pub mod music;

pub struct MusicServices {
    pub music: Arc<dyn DogService<serde_json::Value, MusicParams>>,
}

pub fn configure(
    app: &dog_core::DogApp<serde_json::Value, MusicParams>,
    state: Arc<RustFsState>,
) -> anyhow::Result<MusicServices> {
    let music: Arc<dyn DogService<serde_json::Value, MusicParams>> =
        Arc::new(music::MusicService::new(Arc::clone(&state)));
    app.register_service("music", Arc::clone(&music));

    music::music_shared::register_hooks(app)?;

    Ok(MusicServices { music })
}
