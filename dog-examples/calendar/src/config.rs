//! # Application Configuration (`config.rs`)
//!
//! Loads and applies configuration parameters from environment variables (`.env`)
//! with fallback defaults, adhering to standard DogRS configuration conventions.
//!
//! Exposes:
//! - [`config`]: Registers configuration values onto [`DogAppBuilder`].
//! - [`configure_http`]: Sets `http.host` and `http.port`.
//! - [`configure_ws`]: Sets `ws.public_broadcasts` and `ws.allowed_origins`.

use std::env;
use std::sync::Arc;

use anyhow::Result;
use dog_core::DogAppBuilder;
use serde_json::Value;

use crate::services::CalendarParams;

/// Configures all application settings into the [`DogAppBuilder`].
pub fn config(builder: &mut DogAppBuilder<Value, CalendarParams>) -> Result<()> {
    configure_http(builder)?;
    configure_ws(builder)?;
    Ok(())
}

/// Configures HTTP server host and port.
fn configure_http(builder: &mut DogAppBuilder<Value, CalendarParams>) -> Result<()> {
    let host = env::var("HTTP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("HTTP_PORT").unwrap_or_else(|_| "3036".to_string());

    builder.set("http.host", host);
    builder.set("http.port", port);
    Ok(())
}

/// Configures WebSocket parameters including public broadcast mode and origin security allowlist.
fn configure_ws(builder: &mut DogAppBuilder<Value, CalendarParams>) -> Result<()> {
    let public_broadcasts = env::var("WS_PUBLIC_BROADCASTS").unwrap_or_else(|_| "true".to_string());
    builder.set("ws.public_broadcasts", public_broadcasts);

    let host = env::var("HTTP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("HTTP_PORT").unwrap_or_else(|_| "3036".to_string());
    let default_origin = format!("http://{host}:{port}");

    // Comma-separated allowed origins from .env (e.g. "http://127.0.0.1:3036,http://localhost:3036")
    let raw_origins = env::var("WS_ALLOWED_ORIGINS").unwrap_or_else(|_| {
        format!("{default_origin},http://localhost:3036,http://127.0.0.1:3036")
    });

    let mut origins: Vec<String> = raw_origins
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Ensure the configured host/port origin is always included
    if !origins.contains(&default_origin) {
        origins.push(default_origin);
    }

    builder.set("ws.allowed_origins", Arc::new(origins));
    Ok(())
}
