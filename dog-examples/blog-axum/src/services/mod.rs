use std::sync::Arc;

use dog_core::DogService;

pub mod types;
pub use types::{BlogParams, BlogState};

pub mod posts;
pub mod authors;

pub struct BlogServices {
    pub posts: Arc<dyn DogService<serde_json::Value, BlogParams>>,
    pub authors: Arc<dyn DogService<serde_json::Value, BlogParams>>,
}

pub fn configure(
    app: &dog_core::DogApp<serde_json::Value, BlogParams>,
    state: Arc<BlogState>,
) -> anyhow::Result<BlogServices> {
    let posts: Arc<dyn DogService<serde_json::Value, BlogParams>> = Arc::new(posts::PostsService {
        state: Arc::clone(&state),
    });
    app.register_service("posts", Arc::clone(&posts));
    posts::posts_shared::register_hooks(app)?;

    let authors: Arc<dyn DogService<serde_json::Value, BlogParams>> = Arc::new(authors::AuthorsService {
        state: Arc::clone(&state),
    });
    app.register_service("authors", Arc::clone(&authors));
    authors::authors_shared::register_hooks(app)?;

    Ok(BlogServices { posts, authors })
}