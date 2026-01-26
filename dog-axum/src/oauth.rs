use std::marker::PhantomData;

use axum::extract::{OriginalUri, Query};
use axum::http::HeaderMap;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::params::FromRestParams;
use crate::rest;
use crate::AxumApp;
use crate::DogAxumError;

pub struct OAuthRoutes<Q, F>
where
    Q: Clone + Send + Sync + 'static,
    F: Clone + Send + Sync + 'static,
{
    pub login_path: &'static str,
    pub callback_path: &'static str,
    pub capture_path: Option<&'static str>,

    pub service_name: &'static str,
    pub login_method: &'static str,
    pub callback_method: &'static str,

    pub capture_provider: Option<String>,

    pub callback_payload: F,
    pub http_method: &'static str,

    _marker: PhantomData<fn() -> Q>,
}

impl<Q, F> OAuthRoutes<Q, F>
where
    Q: Clone + Send + Sync + 'static,
    F: Clone + Send + Sync + 'static,
{
    pub fn new(
        login_path: &'static str,
        callback_path: &'static str,
        service_name: &'static str,
        login_method: &'static str,
        callback_method: &'static str,
        callback_payload: F,
    ) -> Self {
        Self {
            login_path,
            callback_path,
            capture_path: None,
            service_name,
            login_method,
            callback_method,
            capture_provider: None,
            callback_payload,
            http_method: "GET",
            _marker: PhantomData,
        }
    }

    pub fn with_capture(mut self, capture_path: &'static str, provider: impl Into<String>) -> Self {
        self.capture_path = Some(capture_path);
        self.capture_provider = Some(provider.into());
        self
    }

    pub fn with_http_method(mut self, method: &'static str) -> Self {
        self.http_method = method;
        self
    }
}

pub fn mount_oauth_routes<P, Q, F>(
    mut ax: AxumApp<Value, P>,
    cfg: OAuthRoutes<Q, F>,
) -> AxumApp<Value, P>
where
    P: FromRestParams + Send + Sync + Clone + 'static,
    Q: DeserializeOwned + serde::Serialize + Clone + Send + Sync + 'static,
    F: Fn(&Q) -> Value + Clone + Send + Sync + 'static,
{
    let app_arc = std::sync::Arc::clone(&ax.app);

    ax = ax.service(
        cfg.login_path,
        {
            let app_arc = std::sync::Arc::clone(&app_arc);
            let service_name = cfg.service_name;
            let method = cfg.login_method;
            let http_method = cfg.http_method;
            move |headers: HeaderMap, OriginalUri(uri): OriginalUri| {
                let app_arc = std::sync::Arc::clone(&app_arc);
                async move {
                    rest::call_custom_redirect_location::<Value, P>(
                        app_arc.as_ref(),
                        service_name,
                        method,
                        &headers,
                        Default::default(),
                        http_method,
                        &uri,
                        None,
                    )
                    .await
                }
            }
        },
    );

    ax = ax.service(
        cfg.callback_path,
        {
            let app_arc = std::sync::Arc::clone(&app_arc);
            let service_name = cfg.service_name;
            let method = cfg.callback_method;
            let http_method = cfg.http_method;
            let payload = cfg.callback_payload.clone();
            move |headers: HeaderMap, Query(query): Query<Q>, OriginalUri(uri): OriginalUri| {
                let app_arc = std::sync::Arc::clone(&app_arc);
                let payload = payload.clone();
                async move {
                    let data = (payload)(&query);
                    rest::call_custom_json_qd::<Value, P, Q, Value>(
                        app_arc.as_ref(),
                        service_name,
                        method,
                        &headers,
                        &query,
                        http_method,
                        &uri,
                        &data,
                    )
                    .await
                }
            }
        },
    );

    if let (Some(capture_path), Some(provider)) = (cfg.capture_path, cfg.capture_provider) {
        ax = ax.service(
            capture_path,
            move |Query(query): Query<Q>| async move {
                Ok::<_, DogAxumError>(rest::oauth_callback_capture_typed(provider.clone(), &query))
            },
        );
    }

    ax
}
