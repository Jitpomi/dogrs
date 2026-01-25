use std::collections::HashMap;

use axum::http::HeaderMap;
use axum::http::Uri;

#[derive(Debug, Clone, Default)]
pub struct RestParams {
    pub provider: String,
    pub headers: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub method: String,
    pub path: String,
    pub raw_query: Option<String>,
}

impl RestParams {
    pub fn from_parts(
        provider: &str,
        headers: &HeaderMap,
        query: HashMap<String, String>,
        method: &str,
        uri: &Uri,
    ) -> Self {
        let mut out = Self {
            provider: provider.to_string(),
            headers: HashMap::new(),
            query,
            method: method.to_string(),
            path: uri.path().to_string(),
            raw_query: uri.query().map(|s| s.to_string()),
        };

        for (k, v) in headers.iter() {
            if let Ok(s) = v.to_str() {
                out.headers.insert(k.to_string(), s.to_string());
            }
        }

        out
    }
}

pub trait FromRestParams: Sized {
    fn from_rest_params(params: RestParams) -> Self;
}

impl FromRestParams for RestParams {
    fn from_rest_params(params: RestParams) -> Self {
        params
    }
}

impl FromRestParams for () {
    fn from_rest_params(_params: RestParams) -> Self {
        
    }
}

#[cfg(feature = "auth")]
impl FromRestParams for dog_auth::hooks::authenticate::AuthParams<RestParams> {
    fn from_rest_params(params: RestParams) -> Self {
        dog_auth::hooks::authenticate::AuthParams {
            inner: params.clone(),
            provider: Some(params.provider.clone()),
            headers: params.headers.clone(),
            authentication: None,
            authenticated: false,
            auth_result: None,
            connection: None,
        }
    }
}
