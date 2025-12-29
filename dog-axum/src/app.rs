use std::sync::Arc;

use axum::body::Body;
use axum::handler::Handler;
use axum::http::Request;
use axum::http::{HeaderName, HeaderValue};
use axum::routing::get;
use axum::Router;
use axum::{middleware, response::Response};
use dog_core::DogApp;
use dog_core::DogService;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::net::{TcpListener, ToSocketAddrs};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::params::FromRestParams;
use crate::rest;
use crate::DogAxumState;

/// Wrapper for service middleware
pub enum ServiceMiddleware<L> {
    Some(L),
    None,
}

impl<L> ServiceMiddleware<L>
where
    L: tower::layer::Layer<axum::routing::Route> + Clone + Send + Sync + 'static,
    L::Service: tower::Service<Request<Body>, Response = Response> + Clone + Send + Sync + 'static,
    <L::Service as tower::Service<Request<Body>>>::Future: Send,
    <L::Service as tower::Service<Request<Body>>>::Error: Into<std::convert::Infallible>,
{
    pub fn apply(self, router: Router<()>) -> Router<()> {
        match self {
            ServiceMiddleware::Some(layer) => router.layer(layer),
            ServiceMiddleware::None => router,
        }
    }
}

impl<L> From<L> for ServiceMiddleware<L>
where
    L: tower::layer::Layer<axum::routing::Route> + Clone + Send + Sync + 'static,
    L::Service: tower::Service<Request<Body>, Response = Response> + Clone + Send + Sync + 'static,
    <L::Service as tower::Service<Request<Body>>>::Future: Send,
    <L::Service as tower::Service<Request<Body>>>::Error: Into<std::convert::Infallible>,
{
    fn from(layer: L) -> Self {
        ServiceMiddleware::Some(layer)
    }
}

async fn ensure_request_id(req: Request<Body>, next: middleware::Next) -> Response {
    let request_id_header = HeaderName::from_static("x-request-id");

    let mut req = req;
    let request_id = req
        .headers()
        .get(&request_id_header)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Ensure the header is available to downstream handlers/middleware.
    if req.headers().get(&request_id_header).is_none() {
        if let Ok(v) = HeaderValue::from_str(&request_id) {
            req.headers_mut().insert(request_id_header.clone(), v);
        }
    }

    let mut res = next.run(req).await;

    if res.headers().get(&request_id_header).is_none() {
        if let Ok(v) = HeaderValue::from_str(&request_id) {
            res.headers_mut().insert(request_id_header, v);
        }
    }

    res
}

fn layer_defaults(router: Router<()>) -> Router<()> {
    router
        .layer(middleware::from_fn(ensure_request_id))
        .layer(TraceLayer::new_for_http())
}

pub struct AxumApp<R, P = ()>
where
    R: Send + Sync + 'static,
    P: Send + Sync + Clone + 'static,
{
    pub app: Arc<DogApp<R, P>>,
    pub router: Router<()>,
    pending_middleware: Vec<Box<dyn Fn(Router<()>) -> Router<()> + Send + Sync>>,
}

impl<R, P> Clone for AxumApp<R, P>
where
    R: Send + Sync + 'static,
    P: Send + Sync + Clone + 'static,
{
    fn clone(&self) -> Self {
        Self {
            app: Arc::clone(&self.app),
            router: self.router.clone(),
            pending_middleware: vec![], // Can't clone closures, so start fresh
        }
    }
}

impl<R, P> AxumApp<R, P>
where
    R: Send + Sync + 'static,
    P: Send + Sync + Clone + 'static,
{
    pub fn new(app: DogApp<R, P>) -> Self {
        let app = Arc::new(app);
        let state = DogAxumState {
            app: Arc::clone(&app),
        };
        Self {
            app,
            router: layer_defaults(Router::new().with_state(state)),
            pending_middleware: vec![],
        }
    }

    pub fn use_router(mut self, path: &str, router: Router<()>) -> Self {
        self.router = layer_defaults(self.router.nest(path, router));
        self
    }

    pub fn r#use(self, path: &str, router: Router<()>) -> Self {
        self.use_router(path, router)
    }

    pub fn use_get<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, ()> + Clone + Send + 'static,
        T: 'static,
    {
        let router = Router::new().route("/", get(handler));
        self.use_router(path, router)
    }

    pub fn service<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, ()> + Clone + Send + 'static,
        T: 'static,
    {
        self.use_get(path, handler)
    }

    pub fn use_service(mut self, path: &'static str, service: Arc<dyn DogService<R, P>>) -> Self
    where
        R: Serialize + DeserializeOwned,
        P: FromRestParams,
    {
        let name = path.trim_start_matches('/');
        self.app.register_service(name, service);

        let service_name = Arc::new(name.to_string());
        let mut router = rest::service_router(Arc::clone(&service_name), Arc::clone(&self.app));

        // Apply pending middleware to this service router
        for middleware_fn in &self.pending_middleware {
            router = middleware_fn(router);
        }

        self.router = layer_defaults(self.router.nest(path, router));
        self
    }

    pub fn use_service_with<L>(mut self, path: &'static str, service: Arc<dyn DogService<R, P>>, middleware: L) -> Self
    where
        R: Serialize + DeserializeOwned,
        P: FromRestParams,
        L: tower::layer::Layer<axum::routing::Route> + Clone + Send + Sync + 'static,
        L::Service: tower::Service<Request<Body>, Response = Response> + Clone + Send + Sync + 'static,
        <L::Service as tower::Service<Request<Body>>>::Future: Send,
        <L::Service as tower::Service<Request<Body>>>::Error: Into<std::convert::Infallible>,
    {
        let name = path.trim_start_matches('/');
        self.app.register_service(name, service);

        let service_name = Arc::new(name.to_string());
        let router = rest::service_router(Arc::clone(&service_name), Arc::clone(&self.app));

        // Apply the specific middleware to this service router
        let router = router.layer(middleware);

        self.router = layer_defaults(self.router.nest(path, router));
        self
    }

    pub fn use_middleware<L>(mut self, layer: L) -> Self
    where
        L: tower::layer::Layer<axum::routing::Route> + Clone + Send + Sync + 'static,
        L::Service: tower::Service<Request<Body>, Response = Response> + Clone + Send + Sync + 'static,
        <L::Service as tower::Service<Request<Body>>>::Future: Send,
        <L::Service as tower::Service<Request<Body>>>::Error: Into<std::convert::Infallible>,
    {
        // Store middleware to be applied to next service
        self.pending_middleware.push(Box::new(move |router| router.layer(layer.clone())));
        self
    }

    pub async fn listen<A>(self, addr: A) -> anyhow::Result<()>
    where
        A: ToSocketAddrs,
    {
        let listener = TcpListener::bind(addr).await?;
        axum::serve(listener, self.router).await?;
        Ok(())
    }
}

pub fn axum<R, P>(app: DogApp<R, P>) -> AxumApp<R, P>
where
    R: Send + Sync + 'static,
    P: Send + Sync + Clone + 'static,
{
    AxumApp::new(app)
}
