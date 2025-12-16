//! # DogRS Configuration
//!
//! DogRS includes a minimal, framework-agnostic configuration
//! system based on a simple string key/value store. This mirrors
//! Feathers' `app.set()` / `app.get()` API and allows applications
//! to layer configuration however they like.
//!
//! ## Setting and reading values
//! ```rust
//! use dog_core::DogApp;
//! let mut app = DogApp::<(), ()>::new();
//!
//! app.set("paginate.default", "10");
//! app.set("paginate.max", "50");
//!
//! assert_eq!(app.get("paginate.default"), Some("10".to_string()));
//! ```
//!
//! ## Environment overrides
//! DogRS core is intentionally environment-agnostic. Applications
//! may choose to load environment variables using any convention.
//!
//! Here is a recommended helper:
//!
//! ```rust
//! use dog_core::DogApp;
//! pub fn load_env_config<R, P>(app: &mut DogApp<R, P>, prefix: &str)
//! where
//!     R: Send + 'static,
//!     P: Send + Clone + 'static,
//! {
//!     for (key, value) in std::env::vars() {
//!         if let Some(stripped) = key.strip_prefix(prefix) {
//!             let normalized = stripped
//!                 .to_lowercase()
//!                 .replace("__", "."); // ADSDOG__PAGINATE__DEFAULT → paginate.default
//!
//!             app.set(normalized, value);
//!         }
//!     }
//! }
//! ```
//!
//! Applications can now override configuration using:
//!
//! ```bash
//! export ADSDOG__PAGINATE__DEFAULT=25
//! ```
//!
//! ## Why this design?
//! - Works in any environment (cloud, edge, P2P, serverless)
//! - No dependency on TOML/JSON/YAML formats
//! - Zero stack lock-in
//! - Multi-tenant friendly
//! - Mirrors Feathers’ configuration style in a Rust-friendly way
//!
//! Higher-level loaders (TOML, JSON, Consul, Vault, etc.) are
//! intentionally kept *out* of DogRS so each application remains
//! free to choose its configuration strategy.

use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct DogConfig {
    values: HashMap<String, String>,
}

impl DogConfig {
    /// Create an empty config store.
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    /// Set a configuration key to a string value.
    ///
    /// Example: app.set("paginate.default", "10")
    pub fn set<K, V>(&mut self, key: K, value: V)
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.values.insert(key.into(), value.into());
    }

    /// Get a configuration value by key.
    ///
    /// Returns None if the key is not present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|s| s.as_str())
    }

    /// Check whether a key is present.
    pub fn has(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }
    pub fn snapshot(&self) -> DogConfigSnapshot {
        DogConfigSnapshot::new(self.values.clone())
    }
}

#[derive(Debug, Clone, Default)]
pub struct DogConfigSnapshot {
    map: HashMap<String, String>,
}

impl DogConfigSnapshot {
    pub(crate) fn new(map: HashMap<String, String>) -> Self {
        Self { map }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(|s| s.as_str())
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        self.map.get(key).cloned()
    }

    pub fn get_usize(&self, key: &str) -> Option<usize> {
        self.get(key).and_then(|v| v.parse::<usize>().ok())
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(|v| v.parse::<bool>().ok())
    }
}
