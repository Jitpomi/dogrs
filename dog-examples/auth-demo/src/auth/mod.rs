use anyhow::Result;
use dog_auth::{AuthOptions, AuthStrategy, AuthenticationService};
use dog_auth_local::LocalStrategy;

use serde_json::Value;
use std::sync::Arc;

use crate::services::AuthDemoParams;

pub mod jwt;
pub mod local;
pub mod oauth2;

pub fn strategies(
    builder: &mut dog_core::DogAppBuilder<Value, AuthDemoParams>,
) -> Result<Arc<dog_auth::AuthServiceAdapter<AuthDemoParams>>> {
    let mut opts = AuthOptions::default();
    opts.strategies = vec![
        AuthStrategy::Jwt,
        AuthStrategy::OAuth,
        AuthStrategy::Custom("local".to_string()),
    ];

    opts.jwt.secret = builder.config_snapshot().get_string("auth.jwt.secret");
    opts.service = builder.config_snapshot().get_string("auth.service");
    opts.entity = builder.config_snapshot().get_string("auth.entity");

    let mut auth_builder = AuthenticationService::builder(builder, Some(opts))?;

    jwt::register_jwt(&mut auth_builder);

    let local_strategy = local::register_local(&mut auth_builder);
    builder.set(
        "auth.local",
        Arc::<LocalStrategy<AuthDemoParams>>::clone(&local_strategy),
    );

    let google_authorize_url = oauth2::google::register_google_oauth(builder, &mut auth_builder)?;
    builder.set("oauth.google.authorize_url", google_authorize_url);

    let auth = Arc::new(AuthenticationService::new(Arc::new(auth_builder.build())));
    let adapter = AuthenticationService::install(builder, auth.clone());

    Ok(adapter)
}
