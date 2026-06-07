
use std::sync::Arc;

use dog_auth::core::AuthenticationStrategy;

use dog_auth_local::{LocalStrategy, LocalStrategyOptions};

pub fn register_local<P: Send + Clone + 'static>(auth: &mut dog_auth::core::AuthenticationBuilder<P>) -> Arc<LocalStrategy<P>> {
    let opts = LocalStrategyOptions {
        username_field: "username".to_string(),
        password_field: "password".to_string(),
        entity_username_field: "username".to_string(),
        entity_password_field: "password".to_string(),
        ..Default::default()
    };
    let strategy = Arc::new(LocalStrategy::new().with_options(opts));
    auth.register("local", strategy.clone() as Arc<dyn AuthenticationStrategy<P>>);
    strategy
}

