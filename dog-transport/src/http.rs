use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use dog_core::{DogApp, DogRequest, DogTransportKind, DogMethod, DogParams, TenantContext, DogError};
use super::{IntoDogService, HttpOptions};
use http::{Request, Response, StatusCode, HeaderValue};
use http_body_util::BodyExt;
use bytes::Bytes;

#[derive(Clone)]
pub struct DogHttpService<R, P>
where
    R: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
    P: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
{
    app: DogApp<R, P>,
    options: HttpOptions,
    service_name: Option<String>,
}

impl<R, P> DogHttpService<R, P>
where
    R: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
    P: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
{
    pub fn new(app: DogApp<R, P>, options: HttpOptions) -> Self {
        Self {
            app,
            options,
            service_name: None,
        }
    }

    pub fn service(&self, name: impl Into<String>) -> Self {
        Self {
            app: self.app.clone(),
            options: self.options.clone(),
            service_name: Some(name.into()),
        }
    }
}

impl<R, P> IntoDogService<HttpOptions> for DogApp<R, P>
where
    R: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
    P: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
{
    type Service = DogHttpService<R, P>;

    fn into_service(self, transport: HttpOptions) -> Self::Service {
        DogHttpService::new(self, transport)
    }
}

impl<R, P, B> tower::Service<Request<B>> for DogHttpService<R, P>
where
    R: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
    P: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
    B: http_body::Body + Send + 'static,
    B::Data: Send,
    B::Error: std::fmt::Display + Send,
{
    type Response = Response<http_body_util::Full<Bytes>>;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let app = self.app.clone();
        let options = self.options.clone();
        let service_name = self.service_name.clone();

        Box::pin(async move {
            let (parts, body) = req.into_parts();

            // 1. Extract request ID
            let req_id_header = options.request_id_header.as_deref().unwrap_or("x-request-id");
            let request_id = parts
                .headers
                .get(req_id_header)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

            // 2. Extract tenant ID
            let tenant_header = options.tenant_header.as_deref().unwrap_or("x-tenant-id");
            let tenant_id = parts
                .headers
                .get(tenant_header)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "default".to_string());
            let tenant = TenantContext::new(tenant_id);

            // 3. Parse Method and Path to extract service name and method kind
            let path = parts.uri.path().trim_matches('/');
            let mut path_parts = path.split('/');
            let (service, id) = if let Some(ref fixed_service) = service_name {
                let id = if let Some(first_segment) = path_parts.next() {
                    if first_segment == fixed_service {
                        path_parts.next().map(|s| s.to_string())
                    } else if !first_segment.is_empty() {
                        Some(first_segment.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                };
                (fixed_service.clone(), id)
            } else {
                let first_segment = path_parts.next().unwrap_or("");
                let matched_service = options.routes.as_ref()
                    .and_then(|r| r.get(first_segment))
                    .cloned();

                if let Some(svc_name) = matched_service {
                    let id = path_parts.next().map(|s| s.to_string());
                    (svc_name, id)
                } else if !first_segment.is_empty() {
                    let id = path_parts.next().map(|s| s.to_string());
                    (first_segment.to_string(), id)
                } else {
                    return Ok(make_error_response(
                        DogError::bad_request("Missing service name in path"),
                        &request_id,
                    ));
                }
            };

            // Determine method based on HTTP method and headers
            let method = if let Some(custom_method) = parts.headers.get("x-service-method").and_then(|h| h.to_str().ok()) {
                DogMethod::Custom(custom_method.to_string())
            } else {
                match parts.method {
                    http::Method::GET => {
                        if id.is_some() {
                            DogMethod::Get
                        } else {
                            DogMethod::Find
                        }
                    }
                    http::Method::POST => DogMethod::Create,
                    http::Method::PUT => DogMethod::Update,
                    http::Method::PATCH => DogMethod::Patch,
                    http::Method::DELETE => DogMethod::Remove,
                    _ => {
                        return Ok(make_error_response(
                            DogError::method_not_allowed(format!("HTTP method {} not supported", parts.method)),
                            &request_id,
                        ));
                    }
                }
            };

            // 4. Parse parameters into DogParams matching RestParams structure
            let mut params_map = HashMap::new();
            params_map.insert("provider".to_string(), serde_json::Value::String("rest".to_string()));
            params_map.insert("method".to_string(), serde_json::Value::String(parts.method.to_string()));
            params_map.insert("path".to_string(), serde_json::Value::String(parts.uri.path().to_string()));

            if let Some(query_str) = parts.uri.query() {
                params_map.insert("raw_query".to_string(), serde_json::Value::String(query_str.to_string()));
                if let Ok(queries) = serde_urlencoded::from_str::<HashMap<String, String>>(query_str) {
                    let mut query_map = HashMap::new();
                    for (k, v) in queries {
                        query_map.insert(k, serde_json::Value::String(v));
                    }
                    params_map.insert("query".to_string(), serde_json::to_value(query_map).unwrap());
                }
            } else {
                params_map.insert("query".to_string(), serde_json::Value::Object(serde_json::Map::new()));
                params_map.insert("raw_query".to_string(), serde_json::Value::Null);
            }

            let mut headers_map = HashMap::new();
            for (k, v) in &parts.headers {
                if let Ok(s) = v.to_str() {
                    headers_map.insert(k.to_string(), s.to_string());
                }
            }
            params_map.insert("headers".to_string(), serde_json::to_value(headers_map).unwrap());

            // Put standard headers into metadata
            let mut metadata = HashMap::new();
            for (k, v) in &parts.headers {
                if let Ok(s) = v.to_str() {
                    metadata.insert(k.to_string(), serde_json::Value::String(s.to_string()));
                }
            }

            // 5. Read body
            let limit = options.body_limit.unwrap_or(10 * 1024 * 1024); // default 10MB
            let body_bytes = match body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(err) => {
                    return Ok(make_error_response(
                        DogError::bad_request(format!("Failed to read request body: {}", err)),
                        &request_id,
                    ));
                }
            };

            if body_bytes.len() > limit {
                return Ok(make_error_response(
                    DogError::new(dog_core::errors::ErrorKind::LengthRequired, "Request body exceeds limit"),
                    &request_id,
                ));
            }

            let payload = if !body_bytes.is_empty() {
                match serde_json::from_slice::<serde_json::Value>(&body_bytes) {
                    Ok(val) => Some(val),
                    Err(err) => {
                        return Ok(make_error_response(
                            DogError::bad_request(format!("Invalid JSON: {}", err)),
                            &request_id,
                        ));
                    }
                }
            } else {
                None
            };

            // 6. Build DogRequest
            let dog_req = DogRequest {
                request_id: Some(request_id.clone()),
                transport: DogTransportKind::Http,
                service,
                method,
                id,
                tenant,
                params: DogParams::from(params_map),
                payload,
                metadata,
            };

            // 7. Dispatch to app.handle()
            match app.handle(dog_req).await {
                Ok(dog_res) => {
                    let mut res = Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json");

                    if let Ok(val) = HeaderValue::from_str(&request_id) {
                        res = res.header("x-request-id", val);
                    }

                    // Handle CORS if enabled
                    if options.enable_cors.unwrap_or(false) {
                        res = res.header("access-control-allow-origin", "*");
                    }

                    let body_val = dog_res.payload.unwrap_or(serde_json::Value::Null);
                    let body_bytes = serde_json::to_vec(&body_val).unwrap_or_default();
                    let response = res.body(http_body_util::Full::new(Bytes::from(body_bytes))).unwrap_or_else(|_| {
                        Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(http_body_util::Full::new(Bytes::new()))
                            .unwrap()
                    });
                    Ok(response)
                }
                Err(err) => {
                    Ok(make_error_response_with_options(err, &request_id, &options))
                }
            }
        })
    }
}

fn make_error_response_with_options(err: DogError, request_id: &str, options: &HttpOptions) -> Response<http_body_util::Full<Bytes>> {
    let status_code = StatusCode::from_u16(err.code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let serialized = serde_json::to_vec(&err.sanitize_for_client()).unwrap_or_default();

    let mut res = Response::builder()
        .status(status_code)
        .header("content-type", "application/json");

    if let Ok(val) = HeaderValue::from_str(request_id) {
        res = res.header("x-request-id", val);
    }

    if options.enable_cors.unwrap_or(false) {
        res = res.header("access-control-allow-origin", "*");
    }

    res.body(http_body_util::Full::new(Bytes::from(serialized)))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(http_body_util::Full::new(Bytes::new()))
                .unwrap()
        })
}

fn make_error_response(err: DogError, request_id: &str) -> Response<http_body_util::Full<Bytes>> {
    make_error_response_with_options(err, request_id, &HttpOptions::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dog_core::{DogApp, DogService, TenantContext};
    use async_trait::async_trait;
    use std::sync::Arc;
    use tower::Service;
    use http::Request;

    #[derive(serde::Serialize, serde::Deserialize, Clone)]
    struct MockData {
        id: Option<String>,
        service: String,
    }

    struct MockService {
        name: String,
    }

    #[async_trait]
    impl DogService<MockData, ()> for MockService {
        async fn get(&self, _ctx: &TenantContext, id: &str, _params: ()) -> anyhow::Result<MockData> {
            Ok(MockData {
                id: Some(id.to_string()),
                service: self.name.clone(),
            })
        }

        async fn find(&self, _ctx: &TenantContext, _params: ()) -> anyhow::Result<Vec<MockData>> {
            Ok(vec![MockData {
                id: None,
                service: self.name.clone(),
            }])
        }
    }

    #[tokio::test]
    async fn test_uniform_and_conventional_routing() {
        let mut builder = DogApp::builder();
        builder.register_service("drivers", Arc::new(MockService { name: "drivers".to_string() }));
        builder.register_service("vehicles", Arc::new(MockService { name: "vehicles".to_string() }));
        let app = builder.build();

        // 1. Test standard/conventional routing (no custom routes map)
        let mut service_default = app.clone().into_service(HttpOptions::default());

        // GET /drivers -> find
        let req = Request::builder().uri("/drivers").body(http_body_util::Empty::<Bytes>::new()).unwrap();
        let res = service_default.call(req).await.unwrap();
        assert_eq!(res.status(), http::StatusCode::OK);

        // GET /drivers/123 -> get
        let req = Request::builder().uri("/drivers/123").body(http_body_util::Empty::<Bytes>::new()).unwrap();
        let res = service_default.call(req).await.unwrap();
        assert_eq!(res.status(), http::StatusCode::OK);

        // 2. Test custom/uniform routing configuration on HttpOptions
        let options = HttpOptions::default()
            .route("/my-drivers-alias", "drivers")
            .route("my-vehicles-alias/", "vehicles");
        let mut service_custom = app.clone().into_service(options);

        // GET /my-drivers-alias -> find (maps to drivers)
        let req = Request::builder().uri("/my-drivers-alias").body(http_body_util::Empty::<Bytes>::new()).unwrap();
        let res = service_custom.call(req).await.unwrap();
        assert_eq!(res.status(), http::StatusCode::OK);

        // GET /my-drivers-alias/456 -> get (maps to drivers, id = 456)
        let req = Request::builder().uri("/my-drivers-alias/456").body(http_body_util::Empty::<Bytes>::new()).unwrap();
        let res = service_custom.call(req).await.unwrap();
        assert_eq!(res.status(), http::StatusCode::OK);

        // GET /my-vehicles-alias/789 -> get (maps to vehicles, id = 789)
        let req = Request::builder().uri("/my-vehicles-alias/789").body(http_body_util::Empty::<Bytes>::new()).unwrap();
        let res = service_custom.call(req).await.unwrap();
        assert_eq!(res.status(), http::StatusCode::OK);
    }
}
