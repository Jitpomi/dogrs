use std::sync::Arc;

use axum::{
    extract::{OriginalUri, Path, Query, State},
    extract::rejection::JsonRejection,
    http::HeaderMap,
    routing,
    Json,
    Router,
};
use dog_core::{tenant::TenantContext, DogApp};
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
                    let res = svc.find(tenant, params).await?;
                    Ok::<_, DogAxumError>(Json(res))
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
                    let res = svc.create(tenant, data, params).await?;
                    Ok::<_, DogAxumError>(Json(res))
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
