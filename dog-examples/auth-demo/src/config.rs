use crate::services::AuthDemoParams;
use anyhow::{anyhow, Result};
use dog_core::DogApp;
use serde_json::Value;
use std::env;

/// Configure all application settings including external APIs and business rules
pub fn config(dog_app: &DogApp<Value, AuthDemoParams>) -> Result<()> {
    // HTTP Server Configuration
    configure_http(dog_app)?;

    // Auth Configuration
    configure_auth(dog_app)?;

    // External API Configuration
    configure_external_apis(dog_app)?;


    Ok(())
}

/*
/// Get configuration value with priority: TypeDB rules > env vars > defaults
pub async fn get_config_value(
    _dog_app: &DogApp<Value, AuthDemoParams>,
    key: &str,
    default: &str,
) -> String {
    // 1. Try environment variables
    if let Ok(env_value) = env::var(key.to_uppercase().replace(".", "_")) {
        return env_value;
    }

    // 3. Use default value
    default.to_string()
}
*/

/// Configure HTTP server settings
fn configure_http(dog_app: &DogApp<Value, AuthDemoParams>) -> Result<()> {
    let host = env::var("HTTP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("HTTP_PORT").unwrap_or_else(|_| "3000".to_string());

    dog_app.set("http.host", host);
    dog_app.set("http.port", port);
    Ok(())
}

/// Configure all authentication parameters
fn configure_auth(dog_app: &DogApp<Value, AuthDemoParams>) -> Result<()> {
    let jwt_secret = env::var("AUTH_JWT_SECRET").unwrap_or_else(|_| "dev-secret".to_string());
    let service = env::var("AUTH_SERVICE").unwrap_or_else(|_| "users".to_string());
    let entity = env::var("AUTH_ENTITY").unwrap_or_else(|_| "user".to_string());

    dog_app.set("auth.jwt.secret", jwt_secret);
    dog_app.set("auth.service", service);
    dog_app.set("auth.entity", entity);
    Ok(())
}

/// Configure external API integrations
fn configure_external_apis(dog_app: &DogApp<Value, AuthDemoParams>) -> Result<()> {
    // Configure Google OAuth
    let google_client_id = env::var("GOOGLE_CLIENT_ID")
        .unwrap_or_default()
        .trim()
        .to_string();
    let google_client_secret = env::var("GOOGLE_CLIENT_SECRET")
        .unwrap_or_default()
        .trim()
        .to_string();
    let google_redirect_uri = env::var("GOOGLE_REDIRECT_URL")
        .or_else(|_| env::var("GOOGLE_REDIRECT_URI"))
        .unwrap_or_default()
        .trim()
        .to_string();

    if google_client_id.is_empty() {
        return Err(anyhow!("Missing GOOGLE_CLIENT_ID"));
    }
    if google_client_secret.is_empty() {
        return Err(anyhow!("Missing GOOGLE_CLIENT_SECRET"));
    }
    if google_redirect_uri.is_empty() {
        return Err(anyhow!("Missing GOOGLE_REDIRECT_URL"));
    }

    dog_app.set("oauth.google.client_id", google_client_id);
    dog_app.set("oauth.google.client_secret", google_client_secret);
    dog_app.set("oauth.google.redirect_uri", google_redirect_uri);

    Ok(())
}

/*
/// Configure all business rule parameters
fn configure_business_rules(_dog_app: &DogApp<Value, AuthDemoParams>) -> Result<()> {


    Ok(())
}
*/
