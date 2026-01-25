use std::sync::Arc;
use anyhow::Result;
use dog_core::{DogApp, DogService};
use serde_json::Value;



use crate::auth;
use dog_auth::AuthenticationService;

pub mod types;
pub use types::AuthDemoParams;

pub mod adapters;
pub mod messages;
pub mod users;
pub mod authentication;

pub struct AuthServices {
    pub messages: Arc<dyn DogService<Value, AuthDemoParams>>,
    pub users: Arc<dyn DogService<Value, AuthDemoParams>>,
    pub auth_svc: Arc<dyn DogService<Value, AuthDemoParams>>,
}

pub fn configure(app: &DogApp<Value, AuthDemoParams>) -> Result<AuthServices> {
    // Create and register message service
    let messages: Arc<dyn DogService<Value, AuthDemoParams>> = Arc::new(messages::MessagesService::new());
    app.register_service("messages", Arc::clone(&messages));
    messages::messages_shared::register_hooks(app)?;

    let users: Arc<dyn DogService<Value, AuthDemoParams>> = Arc::new(users::UsersService::new());
    app.register_service("users", Arc::clone(&users));

    let auth_core = AuthenticationService::from_app(app)
        .ok_or_else(|| anyhow::anyhow!("AuthenticationService missing from app state; did you call AuthenticationService::install?"))?;
    let local = auth::register_local(Arc::clone(&auth_core));

    users::users_shared::register_hooks(app, local)?;

    let auth_svc: Arc<dyn DogService<Value, AuthDemoParams>> = Arc::new(authentication::AuthService::new(auth_core));
    app.register_service("authentication", Arc::clone(&auth_svc));
    authentication::authentication_shared::register_hooks(app)?;


    Ok(AuthServices { messages, users, auth_svc })
}


