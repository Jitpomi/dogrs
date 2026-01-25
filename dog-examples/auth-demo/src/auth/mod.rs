

use anyhow::Result;
use dog_auth::{AuthOptions, AuthStrategy, AuthenticationService};
use dog_auth_local::LocalStrategy;
use dog_core::DogApp;
use serde_json::Value;
use std::sync::Arc;

use crate::services::AuthDemoParams;

pub mod jwt;
pub mod local;
pub mod oauth2;

pub fn strategies(dog_app: &DogApp<Value, AuthDemoParams>) -> Result<()> {
    let mut opts = AuthOptions::default();
    opts.strategies = vec![
        AuthStrategy::Jwt,
        AuthStrategy::OAuth,
        AuthStrategy::Custom("local".to_string()),
    ];

    opts.jwt.secret = dog_app.get::<String>("auth.jwt.secret");
    opts.service = dog_app.get::<String>("auth.service");
    opts.entity = dog_app.get::<String>("auth.entity");

    let auth = Arc::new(AuthenticationService::new(dog_app.clone(), Some(opts))?);
    AuthenticationService::install(dog_app, auth.clone());

    jwt::register_jwt(&auth);

    let local_strategy = local::register_local(Arc::clone(&auth));
    dog_app.set(
        "auth.local",
        Arc::<LocalStrategy<AuthDemoParams>>::clone(&local_strategy),
    );

    let google_authorize_url = oauth2::google::register_google_oauth(Arc::clone(&auth))?;
    dog_app.set("oauth.google.authorize_url", google_authorize_url);

    Ok(())
}
