use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use anyhow::Result;
use dog_core::{DogApp, DogService};
use serde_json::Value;

pub mod types;
pub use types::AuthDemoParams;

pub mod adapters;
pub mod messages;
pub mod users;

#[derive(Default)]
pub struct AuthDemoState {
    pub messages: Mutex<HashMap<String, Value>>,
    pub users: Mutex<HashMap<String, Value>>,
}

pub struct AuthServices {
    pub messages: Arc<dyn DogService<Value, AuthDemoParams>>,
    pub users: Arc<dyn DogService<Value, AuthDemoParams>>,
}

pub fn configure(app: &DogApp<Value, AuthDemoParams>) -> Result<AuthServices> {
    // Create and register message service
    let messages: Arc<dyn DogService<Value, AuthDemoParams>> = Arc::new(messages::MessagesService::new());
    app.register_service("messages", Arc::clone(&messages));
    messages::messages_shared::register_hooks(app)?;

    let users: Arc<dyn DogService<Value, AuthDemoParams>> = Arc::new(users::UsersService::new());
    app.register_service("users", Arc::clone(&users));
    users::users_shared::register_hooks(app)?;

    Ok(AuthServices { messages, users })
}


