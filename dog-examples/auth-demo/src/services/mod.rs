use anyhow::Result;
use dog_core::DogService;
use serde_json::Value;
use std::sync::Arc;



pub mod types;
pub use types::AuthDemoParams;

pub mod adapters;
pub mod authentication;
pub mod messages;
pub mod oauth;
pub mod users;

pub struct AuthServices {
    pub messages: Arc<dyn DogService<Value, AuthDemoParams>>,
    pub users: Arc<dyn DogService<Value, AuthDemoParams>>,
    pub auth_svc: Arc<dyn DogService<Value, AuthDemoParams>>,
    pub oauth: Arc<dyn DogService<Value, AuthDemoParams>>,
    pub oauth_raw: Arc<oauth::OauthService>,
}

pub fn configure(
    builder: &mut dog_core::DogAppBuilder<Value, AuthDemoParams>,
    auth_adapter: Arc<dog_auth::AuthServiceAdapter<AuthDemoParams>>,
) -> Result<AuthServices> {
    let auth_core = auth_adapter.auth().clone();
    // Create and register message service
    let messages: Arc<dyn DogService<Value, AuthDemoParams>> =
        Arc::new(messages::MessagesService::new());
    builder.register_service("messages", Arc::clone(&messages));
    messages::messages_shared::register_hooks(builder, auth_core.clone())?;

    let users: Arc<dyn DogService<Value, AuthDemoParams>> = Arc::new(users::UsersService::new());
    builder.register_service("users", Arc::clone(&users));
    users::users_shared::register_hooks(builder, auth_core.clone())?;

    // Register authentication hooks
    let auth_svc: Arc<dyn DogService<Value, AuthDemoParams>> = auth_adapter as _;
    authentication::authentication_shared::register_hooks(builder)?;

    // Register oauth service
    let oauth_raw = Arc::new(oauth::OauthService::new(auth_core.clone()));
    let oauth: Arc<dyn DogService<Value, AuthDemoParams>> = Arc::clone(&oauth_raw) as _;
    builder.register_service("oauth", Arc::clone(&oauth));
    oauth::oauth_shared::register_hooks(builder)?;

    Ok(AuthServices {
        messages,
        users,
        auth_svc,
        oauth,
        oauth_raw,
    })
}
