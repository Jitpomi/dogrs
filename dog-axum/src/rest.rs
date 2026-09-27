use std::sync::Arc;

use axum::{
    body::Body,
    extract::{OriginalUri, Path},
    http::{HeaderMap, Request},
    response::Redirect,
    routing, Json, Router,
};
use dog_core::errors::DogError;
use dog_core::{tenant::TenantContext, DogApp, DogMethod};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::{params::FromRestParams, DogAxumError};

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
    let encoded = serde_urlencoded::to_string(query).map_err(anyhow::Error::from)?;
    let mut parts = uri.clone().into_parts();
    parts.path_and_query = Some(
        format!(
            "{}{}{}",
            uri.path(),
            if encoded.is_empty() { "" } else { "?" },
            encoded
        )
        .parse()
        .map_err(anyhow::Error::from)?,
    );
    let uri = axum::http::Uri::from_parts(parts).map_err(anyhow::Error::from)?;
    let mut request = Request::builder()
        .method(http_method)
        .uri(uri)
        .body(data)
        .map_err(anyhow::Error::from)?;
    *request.headers_mut() = headers.clone();
    dog_transport::http::call_custom(
        app,
        service_name,
        method,
        request,
        &dog_transport::HttpOptions::default(),
    )
    .await
    .map_err(DogAxumError::from)
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

    axum::http::HeaderValue::from_str(location)
        .map_err(|_| DogError::general_error("Invalid redirect location"))?;
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
    use tower::ServiceExt;
    let transport = dog_transport::http::DogHttpService::new((*app).clone(), Default::default());
    let collection = {
        let service_name = service_name.clone();
        let transport = transport.clone();
        move |OriginalUri(uri): OriginalUri, mut request: Request<Body>| {
            let transport = transport.clone();
            let service = (*service_name).clone();
            async move {
                let method = match request.method().as_str() {
                    "POST" => DogMethod::Create,
                    _ => DogMethod::Find,
                };
                let method = request
                    .headers()
                    .get("x-service-method")
                    .and_then(|v| v.to_str().ok())
                    .map(|v| DogMethod::Custom(v.into()))
                    .unwrap_or(method);
                *request.uri_mut() = uri;
                request
                    .extensions_mut()
                    .insert(dog_transport::http::HttpRoute {
                        service,
                        method,
                        id: None,
                    });
                transport.oneshot(request).await.unwrap().map(Body::new)
            }
        }
    };
    let item =
        move |OriginalUri(uri): OriginalUri, Path(id): Path<String>, mut request: Request<Body>| {
            let transport = transport.clone();
            let service = (*service_name).clone();
            async move {
                let method = match request.method().as_str() {
                    "PUT" => DogMethod::Update,
                    "PATCH" => DogMethod::Patch,
                    "DELETE" => DogMethod::Remove,
                    _ => DogMethod::Get,
                };
                *request.uri_mut() = uri;
                request
                    .extensions_mut()
                    .insert(dog_transport::http::HttpRoute {
                        service,
                        method,
                        id: Some(id),
                    });
                transport.oneshot(request).await.unwrap().map(Body::new)
            }
        };
    Router::new()
        .route("/", routing::get(collection.clone()).post(collection))
        .route(
            "/{id}",
            routing::get(item.clone())
                .put(item.clone())
                .patch(item.clone())
                .delete(item),
        )
}
