use std::sync::Arc;

use axum::{
    extract::{OriginalUri, Path, Query, State},
    http::{HeaderMap, Request},
    response::Redirect,
    routing,
    Json,
    Router,
    body::Body,
};
use dog_core::{tenant::TenantContext, DogApp, ServiceMethodKind};
use dog_core::errors::DogError;
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
    P: FromRestParams + Send + Sync + Clone + 'static,
{
    let tenant = tenant_from_headers(headers);

    let params = RestParams::from_parts("rest", headers, query, http_method, uri);
    let params = P::from_rest_params(params);

    let svc = app.service(service_name)?;
    let res = svc.custom(tenant, method, data, params).await?;
    Ok(serde_json::to_value(res).map_err(|e| anyhow::anyhow!(e))?)
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
    P: FromRestParams + Send + Sync + Clone + 'static,
{
    Ok(Json(
        call_custom(app, service_name, method, headers, query, http_method, uri, data).await?,
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
    P: FromRestParams + Send + Sync + Clone + 'static,
{
    let v = call_custom(app, service_name, method, headers, query, http_method, uri, data).await?;
    let location = v
        .get(location_key)
        .and_then(|x| x.as_str())
        .ok_or_else(|| {
            DogError::bad_request(&format!(
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
    P: FromRestParams + Send + Sync + Clone + 'static,
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
    P: FromRestParams + Send + Sync + Clone + 'static,
    Q: Serialize,
    D: Serialize,
{
    let q = query_to_map(query);
    let body: R = serde_json::from_value(serde_json::to_value(data).map_err(|e| anyhow::anyhow!(e))?)
        .map_err(|e| anyhow::anyhow!(e))?;
    call_custom_json(app, service_name, method, headers, q, http_method, uri, Some(body)).await
}

async fn handle_custom_method<R, P>(
    service_name: &str,
    svc: &dog_core::app::ServiceHandle<R, P>,
    method: &str,
    tenant: TenantContext,
    data: Option<R>,
    params: P,
) -> Result<axum::Json<serde_json::Value>, DogAxumError>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: Send + Sync + Clone + 'static,
{
    // Check if the service declares this custom method in its capabilities
    let capabilities = svc.inner().capabilities();
    
    // Check if any custom method with this name exists in capabilities
    let method_name: Option<&'static str> = capabilities.allowed_methods.iter().find_map(|m| {
        match m {
            ServiceMethodKind::Custom(name) if name.eq_ignore_ascii_case(method) => Some(*name),
            _ => None,
        }
    });
    
    let Some(method_name) = method_name else {
        return Err(DogError::bad_request(&format!(
            "Service '{}' does not support custom method '{}'", 
            service_name, 
            method
        )).into_anyhow().into());
    };
    
    // Call the custom method through the DogRS pipeline so hooks run
    let result = svc.custom(tenant, method_name, data, params).await?;
    let json_result = serde_json::to_value(result).map_err(|e| anyhow::anyhow!(e))?;
    Ok(axum::Json(json_result))
}

pub fn service_router<R, P>(service_name: Arc<String>, app: Arc<DogApp<R, P>>) -> Router<()>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: FromRestParams + Send + Sync + Clone + 'static,
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

                    let params = RestParams::from_parts("rest", &headers, query, "GET", &uri);
                    let params = P::from_rest_params(params);

                    let svc = state.app.service(&service_name)?;
                    
                    // Check for custom method header
                    if let Some(custom_method) = headers.get("x-service-method").and_then(|h| h.to_str().ok()) {
                        return handle_custom_method(&service_name, &svc, custom_method, tenant, None, params).await;
                    }
                    
                    let res = svc.find(tenant, params).await?;
                    Ok::<_, DogAxumError>(Json(serde_json::to_value(res).map_err(|e| anyhow::anyhow!(e))?))
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

                    // Use clean Json extractor - multipart is handled by middleware
                    let body_bytes = axum::body::to_bytes(request.into_body(), 10 * 1024 * 1024).await // 10MB limit for JSON
                        .map_err(|e| anyhow::anyhow!("Failed to read request body: {}", e))?;
                    
                    let data: R = serde_json::from_slice(&body_bytes)
                        .map_err(|e| anyhow::anyhow!("Failed to parse JSON: {}", e))?;

                    let params = RestParams::from_parts("rest", &headers, query, "POST", &uri);
                    let params = P::from_rest_params(params);

                    let svc = state.app.service(&service_name)?;
                    
                    // Check for custom method header
                    if let Some(custom_method) = headers.get("x-service-method").and_then(|h| h.to_str().ok()) {
                        return handle_custom_method(&service_name, &svc, custom_method, tenant, Some(data), params).await;
                    }
                    
                    let res = svc.create(tenant, data, params).await?;
                    Ok::<_, DogAxumError>(Json(serde_json::to_value(res).map_err(|e| anyhow::anyhow!(e))?))
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

                    let params = RestParams::from_parts("rest", &headers, query, "GET", &uri);
                    let params = P::from_rest_params(params);

                    let svc = state.app.service(&service_name)?;
                    let res = svc.get(tenant, &id, params).await?;
                    Ok::<_, DogAxumError>(Json(res))
                }
            })
            .put({
                let service_name = Arc::clone(&service_name);
                move |State(state): State<DogAxumState<R, P>>,
                      headers: HeaderMap,
                      Query(query): Query<std::collections::HashMap<String, String>>,
                      OriginalUri(uri): OriginalUri,
                      Path(id): Path<String>,
                      data: Json<R>| async move {
                    let tenant = tenant_from_headers(&headers);

                    let Json(data) = data;

                    let params = RestParams::from_parts("rest", &headers, query, "PUT", &uri);
                    let params = P::from_rest_params(params);

                    let svc = state.app.service(&service_name)?;
                    let res = svc.update(tenant, &id, data, params).await?;
                    Ok::<_, DogAxumError>(Json(res))
                }
            })
            .patch({
                let service_name = Arc::clone(&service_name);
                move |State(state): State<DogAxumState<R, P>>,
                      headers: HeaderMap,
                      Query(query): Query<std::collections::HashMap<String, String>>,
                      OriginalUri(uri): OriginalUri,
                      Path(id): Path<String>,
                      data: Json<R>| async move {
                    let tenant = tenant_from_headers(&headers);

                    let Json(data) = data;

                    let params = RestParams::from_parts("rest", &headers, query, "PATCH", &uri);
                    let params = P::from_rest_params(params);

                    let svc = state.app.service(&service_name)?;
                    let res = svc.patch(tenant, Some(&id), data, params).await?;
                    Ok::<_, DogAxumError>(Json(res))
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

                    let params = RestParams::from_parts("rest", &headers, query, "DELETE", &uri);
                    let params = P::from_rest_params(params);

                    let svc = state.app.service(&service_name)?;
                    let res = svc.remove(tenant, Some(&id), params).await?;
                    Ok::<_, DogAxumError>(Json(res))
                }
            }),
        )
        .with_state(state)
}
