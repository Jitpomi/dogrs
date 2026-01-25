use anyhow::Result;
use dog_axum::{axum, AxumApp};
use dog_core::DogApp;
use serde_json::Value;
use crate::services::AuthDemoParams;

use std::sync::Arc;

use dog_auth::{AuthOptions, AuthStrategy, AuthenticationService, JwtStrategy};


pub fn auth_app() -> Result<AxumApp<Value, AuthDemoParams>> {
    let dog_app: DogApp<Value, AuthDemoParams> = DogApp::new();

    dog_app.set("http.host", "127.0.0.1");
    dog_app.set("http.port", "3000");

    // Install authentication service (Feathers-like) and register strategies.
    let mut opts = AuthOptions::default();
    opts.strategies = vec![AuthStrategy::Jwt, AuthStrategy::Custom("local".to_string())];
    opts.jwt.secret = Some("dev-secret".to_string());
    opts.service = Some("users".to_string());
    opts.entity = Some("user".to_string());

    let auth = Arc::new(AuthenticationService::new(dog_app.clone(), Some(opts))?);
    AuthenticationService::install(&dog_app, auth.clone());

    auth.register_strategy("jwt", Arc::new(JwtStrategy::new(&auth.base)));

    let ax: AxumApp<Value, AuthDemoParams> = axum(dog_app);
    Ok(ax)
}

