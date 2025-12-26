use std::sync::Arc;

use axum::{
    extract::{OriginalUri, Path, Query, State},
    extract::rejection::JsonRejection,
    http::HeaderMap,
    routing,
    Json,
    Router,
};
use dog_core::{tenant::TenantContext, DogApp, ServiceMethodKind};
use dog_core::errors::DogError;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;

use crate::{
    params::{FromRestParams, RestParams},
    DogAxumError, DogAxumState,
};

fn map_json_rejection(rejection: JsonRejection) -> DogAxumError {
    DogError::bad_request("Failed to parse the request body as JSON")
        .with_errors(json!({"_schema": [rejection.to_string()]}))
        .into_anyhow()
        .into()
}

fn tenant_from_headers(headers: &HeaderMap) -> TenantContext {
    headers
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .map(TenantContext::new)
        .unwrap_or_else(|| TenantContext::new("default"))
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
    let method_supported = capabilities.allowed_methods.iter().any(|m| {
        match m {
            ServiceMethodKind::Custom(name) => name.eq_ignore_ascii_case(method),
            _ => false,
        }
    });
    
    if !method_supported {
        return Err(DogError::bad_request(&format!(
            "Service '{}' does not support custom method '{}'", 
            service_name, 
            method
        )).into_anyhow().into());
    }
    
    // Call the custom method handler
    let result = svc.inner().custom(&tenant, method, data, params).await?;
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
                      data: Result<Json<R>, JsonRejection>| async move {
                    let tenant = tenant_from_headers(&headers);

                    let Json(data) = data.map_err(map_json_rejection)?;

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
                      data: Result<Json<R>, JsonRejection>| async move {
                    let tenant = tenant_from_headers(&headers);

                    let Json(data) = data.map_err(map_json_rejection)?;

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
                      data: Result<Json<R>, JsonRejection>| async move {
                    let tenant = tenant_from_headers(&headers);

                    let Json(data) = data.map_err(map_json_rejection)?;

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
