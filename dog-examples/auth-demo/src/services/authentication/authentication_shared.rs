use std::sync::Arc;

use crate::services::AuthDemoParams;

use std::sync::Arc;
use dog_auth::AuthenticationService;
use dog_auth::core::AuthenticationStrategy;
use dog_auth_local::{LocalStrategy, LocalStrategyOptions};

pub fn register_local<P: Send + Clone + 'static>(auth: Arc<AuthenticationService<P>>) -> Arc<LocalStrategy<P>> {
    let opts = LocalStrategyOptions {
        username_field: "username".to_string(),
        password_field: "password".to_string(),
        entity_username_field: "username".to_string(),
        entity_password_field: "password".to_string(),
        ..Default::default()
    };
    let strategy = Arc::new(LocalStrategy::new(&auth.base).with_options(opts));
    auth.register_strategy("local", strategy.clone() as Arc<dyn AuthenticationStrategy<P>>);
    strategy
}
pub fn register_hooks(app: &dog_core::DogApp<serde_json::Value, AuthDemoParams>) -> anyhow::Result<()> {
    app.service("authentication")?.hooks(|h| {
        h.before_create(Arc::new(super::authentication_service_hooks::LogAuthCreate));
        h.before_remove(Arc::new(super::authentication_service_hooks::LogAuthRemove));
    });
    Ok(())
}
