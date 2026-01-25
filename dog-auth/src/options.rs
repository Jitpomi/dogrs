// Authentication options and configuration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Authentication strategies supported by the system
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthStrategy {
    /// JSON Web Token authentication
    Jwt,
    /// OAuth 2.0 authentication
    OAuth,
    /// API Key authentication
    ApiKey,
    /// Basic authentication (username/password)
    Basic,
    /// Custom authentication strategy
    Custom(String),
}

/// JWT signing algorithms
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum JwtAlgorithm {
    /// HMAC using SHA-256
    HS256,
    /// HMAC using SHA-384
    HS384,
    /// HMAC using SHA-512
    HS512,
    /// RSASSA-PKCS1-v1_5 using SHA-256
    RS256,
    /// RSASSA-PKCS1-v1_5 using SHA-384
    RS384,
    /// RSASSA-PKCS1-v1_5 using SHA-512
    RS512,
    /// ECDSA using P-256 and SHA-256
    ES256,
    /// ECDSA using P-384 and SHA-384
    ES384,
}

impl Default for JwtAlgorithm {
    fn default() -> Self {
        Self::HS256
    }
}

/// Token type for JWT claims
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TokenType {
    /// Access token for API requests
    Access,
    /// Refresh token for obtaining new access tokens
    Refresh,
    /// Identity token containing user information
    Identity,
}

impl Default for TokenType {
    fn default() -> Self {
        Self::Access
    }
}

/// Main authentication configuration
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthOptions {
    /// Enabled authentication strategies
    pub strategies: Vec<AuthStrategy>,
    /// JWT-specific configuration
    pub jwt: JwtOptions,
    /// OAuth provider configurations
    pub oauth_providers: HashMap<String, OAuthProvider>,
    /// API key configuration
    pub api_key: ApiKeyOptions,
}

impl Default for AuthOptions {
    fn default() -> Self {
        Self {
            strategies: vec![AuthStrategy::Jwt],
            jwt: JwtOptions::default(),
            oauth_providers: HashMap::new(),
            api_key: ApiKeyOptions::default(),
        }
    }
}

impl AuthOptions {
    /// Validate the entire authentication configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.strategies.is_empty() {
            return Err("At least one authentication strategy must be enabled".to_string());
        }
        
        // Validate JWT configuration if JWT strategy is enabled
        if self.strategies.contains(&AuthStrategy::Jwt) {
            self.jwt.validate().map_err(|e| format!("JWT validation failed: {}", e))?;
        }
        
        // Validate OAuth providers if OAuth strategy is enabled
        if self.strategies.contains(&AuthStrategy::OAuth) {
            if self.oauth_providers.is_empty() {
                return Err("OAuth strategy is enabled but no OAuth providers are configured".to_string());
            }
            
            for (provider_name, provider) in &self.oauth_providers {
                provider.validate().map_err(|e| format!("OAuth provider '{}' validation failed: {}", provider_name, e))?;
            }
        }
        
        // Validate API key configuration if API key strategy is enabled
        if self.strategies.contains(&AuthStrategy::ApiKey) {
            self.api_key.validate().map_err(|e| format!("API key validation failed: {}", e))?;
        }
        
        // Check for duplicate custom strategy names
        let mut custom_strategies = Vec::new();
        for strategy in &self.strategies {
            if let AuthStrategy::Custom(name) = strategy {
                if custom_strategies.contains(name) {
                    return Err(format!("Duplicate custom authentication strategy: '{}'", name));
                }
                custom_strategies.push(name.clone());
            }
        }
        
        Ok(())
    }
    
    /// Create a new AuthOptions builder
    pub fn builder() -> AuthOptionsBuilder {
        AuthOptionsBuilder::new()
    }
}

/// JWT-specific configuration options
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JwtOptions {
    /// JWT signing algorithm
    pub algorithm: JwtAlgorithm,
    /// Token issuer (iss claim)
    pub issuer: String,
    /// Token audience (aud claim)
    pub audience: Vec<String>,
    /// Access token expiration duration
    #[serde(with = "humantime_serde")]
    pub access_token_expires_in: Duration,
    /// Refresh token expiration duration
    #[serde(with = "humantime_serde")]
    pub refresh_token_expires_in: Duration,
    /// Token type for this configuration
    pub token_type: TokenType,
    /// Custom claims to include in tokens
    pub custom_claims: HashMap<String, serde_json::Value>,
    /// JWT signing secret (for HMAC algorithms)
    pub secret: Option<String>,
    /// Path to private key file (for RSA/ECDSA algorithms)
    pub private_key_path: Option<String>,
    /// Path to public key file (for RSA/ECDSA algorithms)
    pub public_key_path: Option<String>,
}

impl Default for JwtOptions {
    fn default() -> Self {
        Self {
            algorithm: JwtAlgorithm::default(),
            issuer: "dogrs-auth".to_string(),
            audience: vec!["dogrs-api".to_string()],
            access_token_expires_in: Duration::from_secs(3600), // 1 hour
            refresh_token_expires_in: Duration::from_secs(604800), // 7 days
            token_type: TokenType::default(),
            custom_claims: HashMap::new(),
            secret: None,
            private_key_path: None,
            public_key_path: None,
        }
    }
}

impl JwtOptions {
    /// Validate JWT configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.issuer.is_empty() {
            return Err("JWT issuer cannot be empty".to_string());
        }
        
        if self.audience.is_empty() {
            return Err("JWT audience cannot be empty".to_string());
        }
        
        // Check that appropriate keys/secrets are provided for the algorithm
        match self.algorithm {
            JwtAlgorithm::HS256 | JwtAlgorithm::HS384 | JwtAlgorithm::HS512 => {
                if self.secret.is_none() {
                    return Err("HMAC algorithms require a secret".to_string());
                }
            }
            JwtAlgorithm::RS256 | JwtAlgorithm::RS384 | JwtAlgorithm::RS512 |
            JwtAlgorithm::ES256 | JwtAlgorithm::ES384 => {
                if self.private_key_path.is_none() && self.public_key_path.is_none() {
                    return Err("RSA/ECDSA algorithms require key files".to_string());
                }
            }
        }
        
        if self.access_token_expires_in.as_secs() == 0 {
            return Err("Access token expiration must be greater than 0".to_string());
        }
        
        if self.refresh_token_expires_in.as_secs() == 0 {
            return Err("Refresh token expiration must be greater than 0".to_string());
        }
        
        Ok(())
    }
}

/// OAuth provider configuration
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthProvider {
    /// Provider name (e.g., "google", "github")
    pub name: String,
    /// OAuth client ID
    pub client_id: String,
    /// OAuth client secret
    pub client_secret: String,
    /// OAuth authorization URL
    pub auth_url: String,
    /// OAuth token URL
    pub token_url: String,
    /// OAuth redirect URI
    pub redirect_uri: String,
    /// OAuth user info URL (optional)
    pub user_info_url: Option<String>,
    /// OAuth scopes to request
    pub scopes: Vec<String>,
}

impl OAuthProvider {
    /// Validate OAuth provider configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("OAuth provider name cannot be empty".to_string());
        }
        
        if self.client_id.is_empty() {
            return Err("OAuth client ID cannot be empty".to_string());
        }
        
        if self.client_secret.is_empty() {
            return Err("OAuth client secret cannot be empty".to_string());
        }
        
        if self.auth_url.is_empty() {
            return Err("OAuth authorization URL cannot be empty".to_string());
        }
        
        if self.token_url.is_empty() {
            return Err("OAuth token URL cannot be empty".to_string());
        }
        
        if self.redirect_uri.is_empty() {
            return Err("OAuth redirect URI cannot be empty".to_string());
        }
        
        // Validate URLs are properly formatted
        if !self.auth_url.starts_with("http://") && !self.auth_url.starts_with("https://") {
            return Err("OAuth authorization URL must be a valid HTTP/HTTPS URL".to_string());
        }
        
        if !self.token_url.starts_with("http://") && !self.token_url.starts_with("https://") {
            return Err("OAuth token URL must be a valid HTTP/HTTPS URL".to_string());
        }
        
        if let Some(ref user_info_url) = self.user_info_url {
            if !user_info_url.starts_with("http://") && !user_info_url.starts_with("https://") {
                return Err("OAuth user info URL must be a valid HTTP/HTTPS URL".to_string());
            }
        }
        
        Ok(())
    }
}

/// API key authentication options
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKeyOptions {
    /// Header name for API key (e.g., "X-API-Key")
    pub header_name: String,
    /// Query parameter name for API key
    pub query_param: Option<String>,
    /// Whether to require API key for all requests
    pub required: bool,
}

impl Default for ApiKeyOptions {
    fn default() -> Self {
        Self {
            header_name: "X-API-Key".to_string(),
            query_param: Some("api_key".to_string()),
            required: false,
        }
    }
}

impl ApiKeyOptions {
    /// Validate API key configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.header_name.is_empty() {
            return Err("API key header name cannot be empty".to_string());
        }
        
        // Validate header name format (basic check)
        if !self.header_name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err("API key header name must contain only alphanumeric characters, hyphens, and underscores".to_string());
        }
        
        if let Some(ref query_param) = self.query_param {
            if query_param.is_empty() {
                return Err("API key query parameter cannot be empty if specified".to_string());
            }
            
            if !query_param.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err("API key query parameter must contain only alphanumeric characters and underscores".to_string());
            }
        }
        
        Ok(())
    }
}

/// Builder pattern for AuthOptions configuration
#[derive(Clone, Debug, Default)]
pub struct AuthOptionsBuilder {
    strategies: Vec<AuthStrategy>,
    jwt: Option<JwtOptions>,
    oauth_providers: HashMap<String, OAuthProvider>,
    api_key: Option<ApiKeyOptions>,
}

impl AuthOptionsBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Add an authentication strategy
    pub fn strategy(mut self, strategy: AuthStrategy) -> Self {
        self.strategies.push(strategy);
        self
    }
    
    /// Set multiple authentication strategies
    pub fn strategies(mut self, strategies: Vec<AuthStrategy>) -> Self {
        self.strategies = strategies;
        self
    }
    
    /// Configure JWT options
    pub fn jwt(mut self, jwt_options: JwtOptions) -> Self {
        self.jwt = Some(jwt_options);
        self
    }
    
    /// Add an OAuth provider
    pub fn oauth_provider(mut self, name: String, provider: OAuthProvider) -> Self {
        self.oauth_providers.insert(name, provider);
        self
    }
    
    /// Configure API key options
    pub fn api_key(mut self, api_key_options: ApiKeyOptions) -> Self {
        self.api_key = Some(api_key_options);
        self
    }
    
    /// Build the final AuthOptions configuration
    pub fn build(self) -> AuthOptions {
        AuthOptions {
            strategies: if self.strategies.is_empty() {
                vec![AuthStrategy::Jwt]
            } else {
                self.strategies
            },
            jwt: self.jwt.unwrap_or_default(),
            oauth_providers: self.oauth_providers,
            api_key: self.api_key.unwrap_or_default(),
        }
    }
    
    /// Build and validate the AuthOptions configuration
    pub fn build_validated(self) -> Result<AuthOptions, String> {
        let options = self.build();
        options.validate()?;
        Ok(options)
    }
}