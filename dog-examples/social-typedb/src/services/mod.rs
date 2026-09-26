use std::sync::Arc;

use crate::typedb::TypeDBState;
use dog_core::DogService;

pub mod types;
pub use types::SocialParams;

pub mod comments;
pub mod groups;
pub mod organizations;
pub mod persons;
pub mod posts;

pub fn configure(
    app: &mut dog_core::DogAppBuilder<serde_json::Value, SocialParams>,
    state: Arc<TypeDBState>,
) -> anyhow::Result<()> {
    let persons: Arc<dyn DogService<serde_json::Value, SocialParams>> =
        Arc::new(persons::PersonsService::new(Arc::clone(&state)));
    app.register_service("persons", Arc::clone(&persons));
    persons::persons_shared::register_hooks(app)?;

    let organizations: Arc<dyn DogService<serde_json::Value, SocialParams>> =
        Arc::new(organizations::OrganizationsService::new(Arc::clone(&state)));
    app.register_service("organizations", Arc::clone(&organizations));

    let groups: Arc<dyn DogService<serde_json::Value, SocialParams>> =
        Arc::new(groups::GroupsService::new(Arc::clone(&state)));
    app.register_service("groups", Arc::clone(&groups));

    let posts: Arc<dyn DogService<serde_json::Value, SocialParams>> =
        Arc::new(posts::PostsService::new(Arc::clone(&state)));
    app.register_service("posts", Arc::clone(&posts));

    let comments: Arc<dyn DogService<serde_json::Value, SocialParams>> =
        Arc::new(comments::CommentsService::new(Arc::clone(&state)));
    app.register_service("comments", Arc::clone(&comments));

    Ok(())
}
