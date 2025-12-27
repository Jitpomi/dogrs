use std::sync::Arc;

use dog_core::DogService;
use crate::typedb::TypeDBState;

pub mod types;
pub use types::SocialParams;

pub mod persons;
pub mod organizations;
pub mod groups;
pub mod posts;
pub mod comments;

pub struct SocialServices {
    pub persons: Arc<dyn DogService<serde_json::Value, SocialParams>>,
    pub organizations: Arc<dyn DogService<serde_json::Value, SocialParams>>,
    pub groups: Arc<dyn DogService<serde_json::Value, SocialParams>>,
    pub posts: Arc<dyn DogService<serde_json::Value, SocialParams>>,
    pub comments: Arc<dyn DogService<serde_json::Value, SocialParams>>,
}

pub fn configure(
    app: &dog_core::DogApp<serde_json::Value, SocialParams>,
    state: Arc<TypeDBState>,
) -> anyhow::Result<SocialServices> {
 
    let persons: Arc<dyn DogService<serde_json::Value, SocialParams>> = Arc::new(persons::PersonsService::new(Arc::clone(&state)));
    app.register_service("persons", Arc::clone(&persons));
    persons::persons_shared::register_hooks(app)?;

    let organizations: Arc<dyn DogService<serde_json::Value, SocialParams>> = Arc::new(organizations::OrganizationsService::new(Arc::clone(&state)));
    app.register_service("organizations", Arc::clone(&organizations));

    let groups: Arc<dyn DogService<serde_json::Value, SocialParams>> = Arc::new(groups::GroupsService::new(Arc::clone(&state)));
    app.register_service("groups", Arc::clone(&groups));

    let posts: Arc<dyn DogService<serde_json::Value, SocialParams>> = Arc::new(posts::PostsService::new(Arc::clone(&state)));
    app.register_service("posts", Arc::clone(&posts));

    let comments: Arc<dyn DogService<serde_json::Value, SocialParams>> = Arc::new(comments::CommentsService::new(Arc::clone(&state)));
    app.register_service("comments", Arc::clone(&comments));

    Ok(SocialServices { 
        persons,
        organizations,
        groups,
        posts,
        comments,
    })
}
