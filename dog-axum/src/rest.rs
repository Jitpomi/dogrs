use std::sync::Arc;

use axum::{
    body::Body,
    extract::{OriginalUri, Path, Query, State},
    http::{HeaderMap, Request},
    response::Redirect,
    routing, Json, Router,
};
use dog_core::errors::DogError;
use dog_core::{tenant::TenantContext, DogApp, DogRequest, DogTransportKind, DogMethod, DogParams};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::{
    params::{FromRestParams, RestParams},
    DogAxumError, DogAxumState,
};

pub fn tenant_from_headers(headers: &HeaderMap) -> TenantContext {
    headers
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .map(TenantContext::new)
        .unwrap_or_else(|| TenantContext::new("default"))
}

pub async fn call_custom<R, P>(
    app: &DogApp<R, P>,
    service_name: &str,
    method: &'static str,
    headers: &HeaderMap,
    query: std::collections::HashMap<String, String>,
    http_method: &'static str,
    uri: &axum::http::Uri,
    data: Option<R>,
) -> Result<serde_json::Value, DogAxumError>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: FromRestParams + Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    let tenant = tenant_from_headers(headers);
    let request_id = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let rest_params = RestParams::from_parts("rest", headers, query, http_method, uri);
    let params_val = serde_json::to_value(rest_params).map_err(|e| anyhow::anyhow!(e))?;
    let params_map = match params_val {
        serde_json::Value::Object(map) => map.into_iter().collect(),
        _ => std::collections::HashMap::new(),
    };

    let payload = data.map(|d| serde_json::to_value(d).unwrap_or(serde_json::Value::Null));

    let mut metadata = std::collections::HashMap::new();
    for (k, v) in headers.iter() {
        if let Ok(s) = v.to_str() {
            metadata.insert(k.to_string(), serde_json::Value::String(s.to_string()));
        }
    }

    let req = DogRequest {
        request_id: Some(request_id),
        transport: DogTransportKind::Http,
        service: service_name.to_string(),
        method: DogMethod::Custom(method.to_string()),
        id: None,
        tenant,
        params: DogParams::from(params_map),
        payload,
        metadata,
    };

    let res = app.handle(req).await.map_err(|e| DogAxumError::from(e))?;
    Ok(res.payload.unwrap_or(serde_json::Value::Null))
}

pub async fn call_custom_json<R, P>(
    app: &DogApp<R, P>,
    service_name: &str,
    method: &'static str,
    headers: &HeaderMap,
    query: std::collections::HashMap<String, String>,
    http_method: &'static str,
    uri: &axum::http::Uri,
    data: Option<R>,
) -> Result<axum::Json<serde_json::Value>, DogAxumError>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: FromRestParams + Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    Ok(Json(
        call_custom(
            app,
            service_name,
            method,
            headers,
            query,
            http_method,
            uri,
            data,
        )
        .await?,
    ))
}

pub async fn call_custom_redirect<R, P>(
    app: &DogApp<R, P>,
    service_name: &str,
    method: &'static str,
    headers: &HeaderMap,
    query: std::collections::HashMap<String, String>,
    http_method: &'static str,
    uri: &axum::http::Uri,
    data: Option<R>,
    location_key: &'static str,
) -> Result<Redirect, DogAxumError>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: FromRestParams + Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    let v = call_custom(
        app,
        service_name,
        method,
        headers,
        query,
        http_method,
        uri,
        data,
    )
    .await?;
    let location = v
        .get(location_key)
        .and_then(|x| x.as_str())
        .ok_or_else(|| {
            DogError::bad_request(format!(
                "Expected response to include '{}' string field",
                location_key
            ))
            .into_anyhow()
        })?;

    Ok(Redirect::temporary(location))
}

pub async fn call_custom_redirect_location<R, P>(
    app: &DogApp<R, P>,
    service_name: &str,
    method: &'static str,
    headers: &HeaderMap,
    query: std::collections::HashMap<String, String>,
    http_method: &'static str,
    uri: &axum::http::Uri,
    data: Option<R>,
) -> Result<Redirect, DogAxumError>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: FromRestParams + Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    call_custom_redirect(
        app,
        service_name,
        method,
        headers,
        query,
        http_method,
        uri,
        data,
        "location",
    )
    .await
}

pub fn query_to_map<T: Serialize>(query: &T) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let Ok(v) = serde_json::to_value(query) else {
        return out;
    };

    let Some(obj) = v.as_object() else {
        return out;
    };

    for (k, v) in obj {
        let s = match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            _ => None,
        };

        if let Some(s) = s {
            out.insert(k.clone(), s);
        }
    }

    out
}

pub fn oauth_callback_capture<T: Serialize>(
    provider: &'static str,
    query: &T,
) -> axum::Json<serde_json::Value> {
    let q = serde_json::to_value(query).unwrap_or(serde_json::Value::Null);
    let code = q.get("code").cloned().unwrap_or(serde_json::Value::Null);
    let state = q.get("state").cloned().unwrap_or(serde_json::Value::Null);
    Json(serde_json::json!({
        "provider": provider,
        "code": code,
        "state": state,
    }))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OAuthCapture {
    pub provider: String,
    pub code: Option<String>,
    pub state: Option<String>,
}

pub fn oauth_callback_capture_typed<T: Serialize>(
    provider: impl Into<String>,
    query: &T,
) -> axum::Json<OAuthCapture> {
    let provider = provider.into();
    let q = serde_json::to_value(query).unwrap_or(serde_json::Value::Null);
    let code = q
        .get("code")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let state = q
        .get("state")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Json(OAuthCapture {
        provider,
        code,
        state,
    })
}

pub async fn call_custom_json_qd<R, P, Q, D>(
    app: &DogApp<R, P>,
    service_name: &str,
    method: &'static str,
    headers: &HeaderMap,
    query: &Q,
    http_method: &'static str,
    uri: &axum::http::Uri,
    data: &D,
) -> Result<axum::Json<serde_json::Value>, DogAxumError>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: FromRestParams + Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    Q: Serialize,
    D: Serialize,
{
    let q = query_to_map(query);
    let body: R =
        serde_json::from_value(serde_json::to_value(data).map_err(|e| anyhow::anyhow!(e))?)
            .map_err(|e| anyhow::anyhow!(e))?;
    call_custom_json(
        app,
        service_name,
        method,
        headers,
        q,
        http_method,
        uri,
        Some(body),
    )
    .await
}

pub async fn call_custom_redirect_qd<R, P, Q, D>(
    app: &DogApp<R, P>,
    service_name: &str,
    method: &'static str,
    headers: &HeaderMap,
    query: &Q,
    http_method: &'static str,
    uri: &axum::http::Uri,
    data: &D,
) -> Result<Redirect, DogAxumError>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: FromRestParams + Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    Q: Serialize,
    D: Serialize,
{
    let q = query_to_map(query);
    let body: R =
        serde_json::from_value(serde_json::to_value(data).map_err(|e| anyhow::anyhow!(e))?)
            .map_err(|e| anyhow::anyhow!(e))?;
    call_custom_redirect_location(
        app,
        service_name,
        method,
        headers,
        q,
        http_method,
        uri,
        Some(body),
    )
    .await
}

pub async fn call_custom_json_q<R, P, Q>(
    app: &DogApp<R, P>,
    service_name: &str,
    method: &'static str,
    headers: &HeaderMap,
    query: &Q,
    http_method: &'static str,
    uri: &axum::http::Uri,
) -> Result<axum::Json<serde_json::Value>, DogAxumError>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: FromRestParams + Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    Q: Serialize,
{
    let q = query_to_map(query);
    call_custom_json(
        app,
        service_name,
        method,
        headers,
        q,
        http_method,
        uri,
        None,
    )
    .await
}

pub async fn call_custom_redirect_q<R, P, Q>(
    app: &DogApp<R, P>,
    service_name: &str,
    method: &'static str,
    headers: &HeaderMap,
    query: &Q,
    http_method: &'static str,
    uri: &axum::http::Uri,
) -> Result<Redirect, DogAxumError>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: FromRestParams + Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    Q: Serialize,
{
    let q = query_to_map(query);
    call_custom_redirect_location(
        app,
        service_name,
        method,
        headers,
        q,
        http_method,
        uri,
        None,
    )
    .await
}

pub fn service_router<R, P>(service_name: Arc<String>, app: Arc<DogApp<R, P>>) -> Router<()>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: FromRestParams + Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    let state = DogAxumState { app };

    Router::new()
        .route(
            "/",
            routing::get({
                let service_name = Arc::clone(&service_name);
                move |State(state): State<DogAxumState<R, P>>,
                      headers: HeaderMap,
                      Query(query): Query<std::collections::HashMap<String, String>>,
                      OriginalUri(uri): OriginalUri| async move {
                    let tenant = tenant_from_headers(&headers);
                    let request_id = headers
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                    let rest_params = RestParams::from_parts("rest", &headers, query, "GET", &uri);
                    let params_val = serde_json::to_value(rest_params).map_err(|e| anyhow::anyhow!(e))?;
                    let params_map = match params_val {
                        serde_json::Value::Object(map) => map.into_iter().collect(),
                        _ => std::collections::HashMap::new(),
                    };

                    let mut metadata = std::collections::HashMap::new();
                    for (k, v) in &headers {
                        if let Ok(s) = v.to_str() {
                            metadata.insert(k.to_string(), serde_json::Value::String(s.to_string()));
                        }
                    }

                    // Check for custom method header
                    let method = if let Some(custom_method) = headers
                        .get("x-service-method")
                        .and_then(|h| h.to_str().ok())
                    {
                        DogMethod::Custom(custom_method.to_string())
                    } else {
                        DogMethod::Find
                    };

                    let req = DogRequest {
                        request_id: Some(request_id),
                        transport: DogTransportKind::Http,
                        service: (*service_name).clone(),
                        method,
                        id: None,
                        tenant,
                        params: DogParams::from(params_map),
                        payload: None,
                        metadata,
                    };

                    let res = state.app.handle(req).await.map_err(|e| DogAxumError::from(e))?;
                    Ok::<_, DogAxumError>(Json(res.payload.unwrap_or(serde_json::Value::Null)))
                }
            })
            .post({
                let service_name = Arc::clone(&service_name);
                move |State(state): State<DogAxumState<R, P>>,
                      headers: HeaderMap,
                      Query(query): Query<std::collections::HashMap<String, String>>,
                      OriginalUri(uri): OriginalUri,
                      request: Request<Body>| async move {
                    let tenant = tenant_from_headers(&headers);
                    let request_id = headers
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                    let body_bytes = axum::body::to_bytes(request.into_body(), 10 * 1024 * 1024)
                        .await
                        .map_err(|e| anyhow::anyhow!(e))?;

                    let payload = if !body_bytes.is_empty() {
                        let val: serde_json::Value = serde_json::from_slice(&body_bytes).map_err(|e| {
                            dog_core::errors::DogError::bad_request(format!(
                                "Failed to parse JSON: {}",
                                e
                            ))
                            .with_errors(serde_json::json!({
                                "_schema": [e.to_string()]
                            }))
                            .into_anyhow()
                        })?;
                        Some(val)
                    } else {
                        None
                    };

                    let rest_params = RestParams::from_parts("rest", &headers, query, "POST", &uri);
                    let params_val = serde_json::to_value(rest_params).map_err(|e| anyhow::anyhow!(e))?;
                    let params_map = match params_val {
                        serde_json::Value::Object(map) => map.into_iter().collect(),
                        _ => std::collections::HashMap::new(),
                    };

                    let mut metadata = std::collections::HashMap::new();
                    for (k, v) in &headers {
                        if let Ok(s) = v.to_str() {
                            metadata.insert(k.to_string(), serde_json::Value::String(s.to_string()));
                        }
                    }

                    // Check for custom method header
                    let method = if let Some(custom_method) = headers
                        .get("x-service-method")
                        .and_then(|h| h.to_str().ok())
                    {
                        DogMethod::Custom(custom_method.to_string())
                    } else {
                        DogMethod::Create
                    };

                    let req = DogRequest {
                        request_id: Some(request_id),
                        transport: DogTransportKind::Http,
                        service: (*service_name).clone(),
                        method,
                        id: None,
                        tenant,
                        params: DogParams::from(params_map),
                        payload,
                        metadata,
                    };

                    let res = state.app.handle(req).await.map_err(|e| DogAxumError::from(e))?;
                    Ok::<_, DogAxumError>(Json(res.payload.unwrap_or(serde_json::Value::Null)))
                }
            }),
        )
        .route(
            "/{id}",
            routing::get({
                let service_name = Arc::clone(&service_name);
                move |State(state): State<DogAxumState<R, P>>,
                      headers: HeaderMap,
                      Query(query): Query<std::collections::HashMap<String, String>>,
                      OriginalUri(uri): OriginalUri,
                      Path(id): Path<String>| async move {
                    let tenant = tenant_from_headers(&headers);
                    let request_id = headers
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                    let rest_params = RestParams::from_parts("rest", &headers, query, "GET", &uri);
                    let params_val = serde_json::to_value(rest_params).map_err(|e| anyhow::anyhow!(e))?;
                    let params_map = match params_val {
                        serde_json::Value::Object(map) => map.into_iter().collect(),
                        _ => std::collections::HashMap::new(),
                    };

                    let mut metadata = std::collections::HashMap::new();
                    for (k, v) in &headers {
                        if let Ok(s) = v.to_str() {
                            metadata.insert(k.to_string(), serde_json::Value::String(s.to_string()));
                        }
                    }

                    let req = DogRequest {
                        request_id: Some(request_id),
                        transport: DogTransportKind::Http,
                        service: (*service_name).clone(),
                        method: DogMethod::Get,
                        id: Some(id),
                        tenant,
                        params: DogParams::from(params_map),
                        payload: None,
                        metadata,
                    };

                    let res = state.app.handle(req).await.map_err(|e| DogAxumError::from(e))?;
                    Ok::<_, DogAxumError>(Json(res.payload.unwrap_or(serde_json::Value::Null)))
                }
            })
            .put({
                let service_name = Arc::clone(&service_name);
                move |State(state): State<DogAxumState<R, P>>,
                      headers: HeaderMap,
                      Query(query): Query<std::collections::HashMap<String, String>>,
                      OriginalUri(uri): OriginalUri,
                      Path(id): Path<String>,
                      request: Request<Body>| async move {
                    let tenant = tenant_from_headers(&headers);
                    let request_id = headers
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                    let body_bytes = axum::body::to_bytes(request.into_body(), 10 * 1024 * 1024)
                        .await
                        .map_err(|e| anyhow::anyhow!(e))?;

                    let payload = if !body_bytes.is_empty() {
                        let val: serde_json::Value = serde_json::from_slice(&body_bytes).map_err(|e| {
                            dog_core::errors::DogError::bad_request(format!(
                                "Failed to parse JSON: {}",
                                e
                            ))
                            .with_errors(serde_json::json!({
                                "_schema": [e.to_string()]
                            }))
                            .into_anyhow()
                        })?;
                        Some(val)
                    } else {
                        None
                    };

                    let rest_params = RestParams::from_parts("rest", &headers, query, "PUT", &uri);
                    let params_val = serde_json::to_value(rest_params).map_err(|e| anyhow::anyhow!(e))?;
                    let params_map = match params_val {
                        serde_json::Value::Object(map) => map.into_iter().collect(),
                        _ => std::collections::HashMap::new(),
                    };

                    let mut metadata = std::collections::HashMap::new();
                    for (k, v) in &headers {
                        if let Ok(s) = v.to_str() {
                            metadata.insert(k.to_string(), serde_json::Value::String(s.to_string()));
                        }
                    }

                    let req = DogRequest {
                        request_id: Some(request_id),
                        transport: DogTransportKind::Http,
                        service: (*service_name).clone(),
                        method: DogMethod::Update,
                        id: Some(id),
                        tenant,
                        params: DogParams::from(params_map),
                        payload,
                        metadata,
                    };

                    let res = state.app.handle(req).await.map_err(|e| DogAxumError::from(e))?;
                    Ok::<_, DogAxumError>(Json(res.payload.unwrap_or(serde_json::Value::Null)))
                }
            })
            .patch({
                let service_name = Arc::clone(&service_name);
                move |State(state): State<DogAxumState<R, P>>,
                      headers: HeaderMap,
                      Query(query): Query<std::collections::HashMap<String, String>>,
                      OriginalUri(uri): OriginalUri,
                      Path(id): Path<String>,
                      request: Request<Body>| async move {
                    let tenant = tenant_from_headers(&headers);
                    let request_id = headers
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                    let body_bytes = axum::body::to_bytes(request.into_body(), 10 * 1024 * 1024)
                        .await
                        .map_err(|e| anyhow::anyhow!(e))?;

                    let payload = if !body_bytes.is_empty() {
                        let val: serde_json::Value = serde_json::from_slice(&body_bytes).map_err(|e| {
                            dog_core::errors::DogError::bad_request(format!(
                                "Failed to parse JSON: {}",
                                e
                            ))
                            .with_errors(serde_json::json!({
                                "_schema": [e.to_string()]
                            }))
                            .into_anyhow()
                        })?;
                        Some(val)
                    } else {
                        None
                    };

                    let rest_params = RestParams::from_parts("rest", &headers, query, "PATCH", &uri);
                    let params_val = serde_json::to_value(rest_params).map_err(|e| anyhow::anyhow!(e))?;
                    let params_map = match params_val {
                        serde_json::Value::Object(map) => map.into_iter().collect(),
                        _ => std::collections::HashMap::new(),
                    };

                    let mut metadata = std::collections::HashMap::new();
                    for (k, v) in &headers {
                        if let Ok(s) = v.to_str() {
                            metadata.insert(k.to_string(), serde_json::Value::String(s.to_string()));
                        }
                    }

                    let req = DogRequest {
                        request_id: Some(request_id),
                        transport: DogTransportKind::Http,
                        service: (*service_name).clone(),
                        method: DogMethod::Patch,
                        id: Some(id),
                        tenant,
                        params: DogParams::from(params_map),
                        payload,
                        metadata,
                    };

                    let res = state.app.handle(req).await.map_err(|e| DogAxumError::from(e))?;
                    Ok::<_, DogAxumError>(Json(res.payload.unwrap_or(serde_json::Value::Null)))
                }
            })
            .delete({
                let service_name = Arc::clone(&service_name);
                move |State(state): State<DogAxumState<R, P>>,
                      headers: HeaderMap,
                      Query(query): Query<std::collections::HashMap<String, String>>,
                      OriginalUri(uri): OriginalUri,
                      Path(id): Path<String>| async move {
                    let tenant = tenant_from_headers(&headers);
                    let request_id = headers
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                    let rest_params = RestParams::from_parts("rest", &headers, query, "DELETE", &uri);
                    let params_val = serde_json::to_value(rest_params).map_err(|e| anyhow::anyhow!(e))?;
                    let params_map = match params_val {
                        serde_json::Value::Object(map) => map.into_iter().collect(),
                        _ => std::collections::HashMap::new(),
                    };

                    let mut metadata = std::collections::HashMap::new();
                    for (k, v) in &headers {
                        if let Ok(s) = v.to_str() {
                            metadata.insert(k.to_string(), serde_json::Value::String(s.to_string()));
                        }
                    }

                    let req = DogRequest {
                        request_id: Some(request_id),
                        transport: DogTransportKind::Http,
                        service: (*service_name).clone(),
                        method: DogMethod::Remove,
                        id: Some(id),
                        tenant,
                        params: DogParams::from(params_map),
                        payload: None,
                        metadata,
                    };

                    let res = state.app.handle(req).await.map_err(|e| DogAxumError::from(e))?;
                    Ok::<_, DogAxumError>(Json(res.payload.unwrap_or(serde_json::Value::Null)))
                }
            }),
        )
        .with_state(state)
}
