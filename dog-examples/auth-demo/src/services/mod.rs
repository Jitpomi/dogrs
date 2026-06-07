use std::sync::Arc;
use anyhow::Result;
use dog_core::DogService;
use serde_json::Value;



use dog_auth::AuthenticationService;

pub mod types;
pub use types::AuthDemoParams;

pub mod adapters;
pub mod messages;
pub mod users;
pub mod authentication;
pub mod oauth;

pub struct AuthServices {
    pub messages: Arc<dyn DogService<Value, AuthDemoParams>>,
    pub users: Arc<dyn DogService<Value, AuthDemoParams>>,
    pub auth_svc: Arc<dyn DogService<Value, AuthDemoParams>>,
    pub oauth: Arc<dyn DogService<Value, AuthDemoParams>>,
    pub oauth_raw: Arc<oauth::OauthService>,
}

pub fn configure(builder: &mut dog_core::DogAppBuilder<Value, AuthDemoParams>, auth_core: Arc<AuthenticationService<AuthDemoParams>>) -> Result<AuthServices> {
    // Create and register message service
    let messages: Arc<dyn DogService<Value, AuthDemoParams>> = Arc::new(messages::MessagesService::new());
    builder.register_service("messages", Arc::clone(&messages));
    messages::messages_shared::register_hooks(builder, auth_core.clone())?;

    let users: Arc<dyn DogService<Value, AuthDemoParams>> = Arc::new(users::UsersService::new());
    builder.register_service("users", Arc::clone(&users));
    users::users_shared::register_hooks(builder, auth_core.clone())?;

    // Register authentication service
    let auth_svc: Arc<dyn DogService<Value, AuthDemoParams>> = Arc::new(authentication::AuthService::new(auth_core.clone()));
    builder.register_service("authentication", Arc::clone(&auth_svc));
    authentication::authentication_shared::register_hooks(builder)?;

    // Register oauth service
    let oauth_raw = Arc::new(oauth::OauthService::new(auth_core.clone()));
    let oauth: Arc<dyn DogService<Value, AuthDemoParams>> = Arc::clone(&oauth_raw) as _;
    builder.register_service("oauth", Arc::clone(&oauth));
    oauth::oauth_shared::register_hooks(builder)?;


    Ok(AuthServices { messages, users, auth_svc, oauth, oauth_raw })
}

