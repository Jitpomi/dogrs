use std::collections::HashMap;

/// Simple application-level configuration store for DogRS.
///
/// This is intentionally string-based and framework-agnostic.
/// Higher-level layers (apps) can decide how to map env vars,
/// JSON, TOML, etc. into these keys and values.
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
}
