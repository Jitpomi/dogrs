// Authentication core.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use dog_core::errors::DogError;
use dog_core::DogApp;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::options::{AuthOptions, TokenType};

#[cfg(any(feature = "jwt-aws-lc-rs", feature = "jwt-rust-crypto"))]
use crate::options::JwtAlgorithm;

pub type AuthenticationResult = Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionEvent {
    Login,
    Logout,
    Disconnect,
}

pub fn extract_bearer_token(headers: &HashMap<String, String>) -> Option<String> {
    let v = headers
        .get("authorization")
        .or_else(|| headers.get("Authorization"))?;
    let v = v.trim();
    let prefix = "Bearer ";
    if v.len() <= prefix.len() || !v.starts_with(prefix) {
        return None;
    }
    Some(v[prefix.len()..].trim().to_string())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AuthenticationRequest {
    pub strategy: Option<String>,
    #[serde(flatten)]
    pub data: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AuthenticationParams {
    pub payload: Option<Value>,
    pub jwt_options: Option<JwtOverrides>,
    pub auth_strategies: Option<Vec<String>>,
    pub secret: Option<String>,
    pub headers: HashMap<String, String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct JwtOverrides {
    pub issuer: Option<String>,
    pub audience: Option<Vec<String>>,
    pub expires_in_seconds: Option<u64>,
    pub token_type: Option<TokenType>,
}

pub trait JwtProvider: Send + Sync {
    fn sign(
        &self,
        jwt: &crate::options::JwtOptions,
        claims: Map<String, Value>,
        token_type: TokenType,
    ) -> Result<String>;

    fn verify(
        &self,
        jwt: &crate::options::JwtOptions,
        token: &str,
        overrides: Option<&JwtOverrides>,
    ) -> Result<Value>;
}

#[cfg(not(any(feature = "jwt-aws-lc-rs", feature = "jwt-rust-crypto")))]
struct NoJwtProvider;

#[cfg(not(any(feature = "jwt-aws-lc-rs", feature = "jwt-rust-crypto")))]
impl JwtProvider for NoJwtProvider {
    fn sign(
        &self,
        _jwt: &crate::options::JwtOptions,
        _claims: Map<String, Value>,
        _token_type: TokenType,
    ) -> Result<String> {
        Err(anyhow::anyhow!(
            "JWT support is disabled (enable one of: jwt-aws-lc-rs, jwt-rust-crypto)"
        ))
    }

    fn verify(
        &self,
        _jwt: &crate::options::JwtOptions,
        _token: &str,
        _overrides: Option<&JwtOverrides>,
    ) -> Result<Value> {
        Err(anyhow::anyhow!(
            "JWT support is disabled (enable one of: jwt-aws-lc-rs, jwt-rust-crypto)"
        ))
    }
}

#[cfg(any(feature = "jwt-aws-lc-rs", feature = "jwt-rust-crypto"))]
struct JsonwebtokenProvider;

#[cfg(any(feature = "jwt-aws-lc-rs", feature = "jwt-rust-crypto"))]
impl JsonwebtokenProvider {
    fn algorithm(alg: JwtAlgorithm) -> jsonwebtoken::Algorithm {
        match alg {
            JwtAlgorithm::HS256 => jsonwebtoken::Algorithm::HS256,
            JwtAlgorithm::HS384 => jsonwebtoken::Algorithm::HS384,
            JwtAlgorithm::HS512 => jsonwebtoken::Algorithm::HS512,
            JwtAlgorithm::RS256 => jsonwebtoken::Algorithm::RS256,
            JwtAlgorithm::RS384 => jsonwebtoken::Algorithm::RS384,
            JwtAlgorithm::RS512 => jsonwebtoken::Algorithm::RS512,
            JwtAlgorithm::ES256 => jsonwebtoken::Algorithm::ES256,
            JwtAlgorithm::ES384 => jsonwebtoken::Algorithm::ES384,
        }
    }
}

#[cfg(any(feature = "jwt-aws-lc-rs", feature = "jwt-rust-crypto"))]
impl JwtProvider for JsonwebtokenProvider {
    fn sign(
        &self,
        jwt: &crate::options::JwtOptions,
        claims: Map<String, Value>,
        token_type: TokenType,
    ) -> Result<String> {
        use jsonwebtoken::{encode, EncodingKey, Header};

        // Minimal implementation: sign using HMAC secret.
        // Key-based algorithms can be added later without changing the public API.
        let secret = jwt.secret.as_ref().ok_or_else(|| {
            DogError::not_authenticated("JWT secret is not configured").into_anyhow()
        })?;

        let mut header = Header::new(Self::algorithm(jwt.algorithm.clone()));
        header.typ = Some(match token_type {
            TokenType::Access => "access",
            TokenType::Refresh => "refresh",
            TokenType::Identity => "identity",
        }
        .to_string());

        encode(&header, &claims, &EncodingKey::from_secret(secret.as_bytes()))
            .map_err(|e| DogError::not_authenticated(e.to_string()).into_anyhow())
    }

    fn verify(
        &self,
        jwt: &crate::options::JwtOptions,
        token: &str,
        overrides: Option<&JwtOverrides>,
    ) -> Result<Value> {
        use jsonwebtoken::{decode, DecodingKey, Validation};

        let secret = jwt.secret.as_ref().ok_or_else(|| {
            DogError::not_authenticated("JWT secret is not configured").into_anyhow()
        })?;

        let issuer = overrides
            .and_then(|o| o.issuer.clone())
            .unwrap_or_else(|| jwt.issuer.clone());
        let audience = overrides
            .and_then(|o| o.audience.clone())
            .unwrap_or_else(|| jwt.audience.clone());

        let alg = Self::algorithm(jwt.algorithm.clone());

        let mut validation = Validation::new(alg);
        validation.set_issuer(&[issuer.as_str()]);
        validation.set_audience(&audience.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        let decoded = decode::<Value>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        )
        .map_err(|e| DogError::not_authenticated(e.to_string()).into_anyhow())?;

        Ok(decoded.claims)
    }
}

#[async_trait]
pub trait AuthenticationStrategy<P>: Send + Sync
where
    P: Send + Clone + 'static,
{
    async fn authenticate(
        &self,
        authentication: &AuthenticationRequest,
        params: &AuthenticationParams,
        app: &DogApp<Value, P>,
    ) -> Result<AuthenticationResult>;
}

pub struct AuthenticationBase<P>
where
    P: Send + Clone + 'static,
{
    app: DogApp<Value, P>,
    config_key: String,
    strategies: RwLock<HashMap<String, Arc<dyn AuthenticationStrategy<P>>>>,
    is_ready: RwLock<bool>,
    jwt: Arc<dyn JwtProvider>,
}

impl<P> AuthenticationBase<P>
where
    P: Send + Clone + 'static,
{
    pub fn new(app: DogApp<Value, P>, config_key: impl Into<String>, options: Option<AuthOptions>) -> Result<Self> {
        let config_key = config_key.into();

        // Store AuthOptions as typed any-state. We keep it as Arc so callers can cheaply clone.
        if let Some(opts) = options {
            app.set(&config_key, Arc::new(opts));
        } else if app.get::<Arc<AuthOptions>>(&config_key).is_none() {
            app.set(&config_key, Arc::new(AuthOptions::default()));
        }

        let jwt: Arc<dyn JwtProvider> = {
            #[cfg(any(feature = "jwt-aws-lc-rs", feature = "jwt-rust-crypto"))]
            {
                Arc::new(JsonwebtokenProvider)
            }
            #[cfg(not(any(feature = "jwt-aws-lc-rs", feature = "jwt-rust-crypto")))]
            {
                Arc::new(NoJwtProvider)
            }
        };

        Ok(Self {
            app,
            config_key,
            strategies: RwLock::new(HashMap::new()),
            is_ready: RwLock::new(false),
            jwt,
        })
    }

    pub fn app(&self) -> &DogApp<Value, P> {
        &self.app
    }

    pub fn configuration(&self) -> AuthOptions {
        self.app
            .get::<Arc<AuthOptions>>(&self.config_key)
            .map(|a| (*a).clone())
            .unwrap_or_default()
    }

    pub fn set_configuration(&self, options: AuthOptions) {
        self.app.set(&self.config_key, Arc::new(options));
    }

    pub fn register(&self, name: impl Into<String>, strategy: Arc<dyn AuthenticationStrategy<P>>) {
        self.strategies.write().unwrap().insert(name.into(), strategy);
    }

    pub fn strategy_names(&self) -> Vec<String> {
        self.strategies
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    pub fn get_strategy(&self, name: &str) -> Option<Arc<dyn AuthenticationStrategy<P>>> {
        self.strategies.read().unwrap().get(name).cloned()
    }

    pub async fn authenticate(
        &self,
        authentication: &AuthenticationRequest,
        params: &AuthenticationParams,
        allowed: &[String],
    ) -> Result<AuthenticationResult> {
        let Some(strategy) = authentication.strategy.as_deref() else {
            return Err(DogError::not_authenticated("Invalid authentication information (no `strategy` set)")
                .into_anyhow());
        };

        if !allowed.is_empty() && !allowed.iter().any(|s| s == strategy) {
            return Err(DogError::not_authenticated(
                "Invalid authentication information (strategy not allowed in authStrategies)",
            )
            .into_anyhow());
        }

        let strat = self.get_strategy(strategy).ok_or_else(|| {
            DogError::not_authenticated("Invalid authentication information")
                .into_anyhow()
        })?;

        strat.authenticate(authentication, params, &self.app).await
    }

    pub async fn setup(&self) {
        *self.is_ready.write().unwrap() = true;
    }

    pub fn is_ready(&self) -> bool {
        *self.is_ready.read().unwrap()
    }

    pub async fn create_access_token(&self, payload: Value, overrides: Option<JwtOverrides>) -> Result<String> {
        self.create_token(payload, overrides, TokenType::Access).await
    }

    pub async fn create_refresh_token(&self, payload: Value, overrides: Option<JwtOverrides>) -> Result<String> {
        self.create_token(payload, overrides, TokenType::Refresh).await
    }

    pub async fn verify_access_token(&self, token: &str) -> Result<Value> {
        self.verify_token(token, None).await
    }

    async fn create_token(&self, payload: Value, overrides: Option<JwtOverrides>, default_type: TokenType) -> Result<String> {
        let cfg = self.configuration();
        let jwt = cfg.jwt;

        let issuer = overrides
            .as_ref()
            .and_then(|o| o.issuer.clone())
            .unwrap_or_else(|| jwt.issuer.clone());
        let audience = overrides
            .as_ref()
            .and_then(|o| o.audience.clone())
            .unwrap_or_else(|| jwt.audience.clone());

        let token_type = overrides
            .as_ref()
            .and_then(|o| o.token_type.clone())
            .unwrap_or(default_type);

        let expires_in_seconds = overrides
            .as_ref()
            .and_then(|o| o.expires_in_seconds)
            .unwrap_or_else(|| match token_type {
                TokenType::Access | TokenType::Identity => jwt.access_token_expires_in.as_secs(),
                TokenType::Refresh => jwt.refresh_token_expires_in.as_secs(),
            });

        let now = Utc::now().timestamp();
        let exp = now + (expires_in_seconds as i64);
        let jti = Uuid::new_v4().to_string();

        let mut claims = match payload {
            Value::Object(m) => m,
            other => {
                let mut m = Map::new();
                m.insert("payload".to_string(), other);
                m
            }
        };

        // Standard-ish fields
        claims.insert("iss".to_string(), Value::String(issuer));
        claims.insert("aud".to_string(), json!(audience));
        claims.insert("iat".to_string(), Value::Number(now.into()));
        claims.insert("exp".to_string(), Value::Number(exp.into()));
        claims.insert("jti".to_string(), Value::String(jti));

        // Merge custom claims from config (config overrides payload)
        for (k, v) in jwt.custom_claims.clone() {
            claims.insert(k, v);
        }

        self.jwt.sign(&jwt, claims, token_type)
    }

    async fn verify_token(&self, token: &str, overrides: Option<JwtOverrides>) -> Result<Value> {
        let cfg = self.configuration();
        let jwt = cfg.jwt;
        self.jwt.verify(&jwt, token, overrides.as_ref())
    }
}