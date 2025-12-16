use std::sync::Arc;

use dog_core::DogService;

pub mod types;
pub use types::{BlogParams, BlogState};

pub mod posts;

pub fn configure(
    app: &dog_core::DogApp<serde_json::Value, BlogParams>,
    state: Arc<BlogState>,
) -> anyhow::Result<Arc<dyn DogService<serde_json::Value, BlogParams>>> {
    let posts: Arc<dyn DogService<serde_json::Value, BlogParams>> = Arc::new(posts::PostsService { state });
    app.register_service("posts", Arc::clone(&posts));
    posts::posts_shared::register_hooks(app)?;
    Ok(posts)
}