// JWT strategy.

use std::sync::{Arc, Weak};

use anyhow::Result;
use async_trait::async_trait;
use dog_core::errors::DogError;
use dog_core::DogApp;
use serde_json::{json, Map, Value};

use crate::core::{AuthenticationBase, AuthenticationParams, AuthenticationRequest, AuthenticationResult, AuthenticationStrategy};

#[derive(Clone, Debug)]
pub struct JwtStrategyOptions {
    pub header: String,
    pub schemes: Vec<String>,
}

impl Default for JwtStrategyOptions {
    fn default() -> Self {
        Self {
            header: "authorization".to_string(),
            schemes: vec!["Bearer".to_string(), "JWT".to_string()],
        }
    }
}

pub struct JwtStrategy<P>
where
    P: Send + Clone + 'static,
{
    auth: Weak<AuthenticationBase<P>>,
    name: String,
    options: JwtStrategyOptions,
}

impl<P> JwtStrategy<P>
where
    P: Send + Clone + 'static,
{
    pub fn new(auth: &Arc<AuthenticationBase<P>>) -> Self {
        Self {
            auth: Arc::downgrade(auth),
            name: "jwt".to_string(),
            options: JwtStrategyOptions::default(),
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn with_options(mut self, options: JwtStrategyOptions) -> Self {
        self.options = options;
        self
    }

    fn parse_from_headers(&self, headers: &std::collections::HashMap<String, String>) -> Option<String> {
        let hv = headers
            .get(&self.options.header)
            .or_else(|| headers.get(&self.options.header.to_lowercase()))
            .or_else(|| headers.get(&self.options.header.to_uppercase()))
            .or_else(|| headers.get("authorization"))
            .or_else(|| headers.get("Authorization"))?;

        let hv = hv.trim();
        if hv.is_empty() {
            return None;
        }

        // Match `<scheme> <token>`.
        if let Some((scheme, token)) = hv.split_once(' ') {
            let scheme = scheme.trim();
            let token = token.trim();
            if token.is_empty() {
                return None;
            }

            let allowed = self
                .options
                .schemes
                .iter()
                .any(|s| s.eq_ignore_ascii_case(scheme));
            if allowed {
                return Some(token.to_string());
            }

            return None;
        }

        // If no scheme, treat whole header as token.
        Some(hv.to_string())
    }

    fn parse_from_request(&self, req: &AuthenticationRequest) -> Option<String> {
        req.data
            .get("accessToken")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty())
    }
}

#[async_trait]
impl<P> AuthenticationStrategy<P> for JwtStrategy<P>
where
    P: Send + Clone + 'static,
{
    async fn authenticate(
        &self,
        authentication: &AuthenticationRequest,
        params: &AuthenticationParams,
        _app: &DogApp<Value, P>,
    ) -> Result<AuthenticationResult> {
        let auth = self
            .auth
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("AuthenticationBase was dropped"))?;

        let access_token = self
            .parse_from_request(authentication)
            .or_else(|| self.parse_from_headers(&params.headers))
            .ok_or_else(|| DogError::not_authenticated("No access token").into_anyhow())?;

        let payload = auth
            .verify_access_token(&access_token)
            .await
            .map_err(|e| DogError::not_authenticated(e.to_string()).into_anyhow())?;

        let mut auth_obj = Map::new();
        auth_obj.insert("strategy".to_string(), Value::String(self.name.clone()));
        auth_obj.insert("accessToken".to_string(), Value::String(access_token.clone()));
        auth_obj.insert("payload".to_string(), payload.clone());

        Ok(json!({
            "accessToken": access_token,
            "authentication": Value::Object(auth_obj),
            "payload": payload
        }))
    }
}