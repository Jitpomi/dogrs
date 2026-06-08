use std::sync::Arc;

use dog_core::DogService;

pub mod types;
pub use types::{BlogParams, BlogState};

pub mod adapters;

pub mod authors;
pub mod posts;

pub struct BlogServices {
    pub posts: Arc<dyn DogService<serde_json::Value, BlogParams>>,
    pub authors: Arc<dyn DogService<serde_json::Value, BlogParams>>,
}

pub fn configure(
    app: &mut dog_core::DogAppBuilder<serde_json::Value, BlogParams>,
    state: Arc<BlogState>,
) -> anyhow::Result<BlogServices> {
    let posts: Arc<dyn DogService<serde_json::Value, BlogParams>> =
        Arc::new(posts::PostsService::new(Arc::clone(&state)));
    app.register_service("posts", Arc::clone(&posts));
    posts::posts_shared::register_hooks(app)?;

    let authors: Arc<dyn DogService<serde_json::Value, BlogParams>> =
        Arc::new(authors::AuthorsService::new(Arc::clone(&state)));
    app.register_service("authors", Arc::clone(&authors));
    authors::authors_shared::register_hooks(app)?;

    Ok(BlogServices { posts, authors })
}
