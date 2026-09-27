//! Unary gRPC adapter for the same request envelope used by HTTP and Iroh.
//! Use `into_server` to compose with your own TLS, interceptors and shutdown policy.
use crate::{GrpcOptions, IntoDogService};
use dog_core::{DogApp, DogError, DogRequest, DogTransportKind};
use serde::{de::DeserializeOwned, Serialize};
use tonic::{Request, Response, Status};

pub mod proto {
    tonic::include_proto!("dog.v1");
    pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("dog_descriptor");
}

pub struct DogGrpcService<R: Send + 'static, P: Send + Clone + 'static> {
    app: DogApp<R, P>,
    options: GrpcOptions,
}

impl<R, P> IntoDogService<GrpcOptions> for DogApp<R, P>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    type Service = DogGrpcService<R, P>;
    fn into_service(self, options: GrpcOptions) -> Self::Service {
        DogGrpcService { app: self, options }
    }
}

impl<R, P> DogGrpcService<R, P>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    pub fn into_server(self) -> proto::dog_transport_server::DogTransportServer<Self> {
        proto::dog_transport_server::DogTransportServer::new(self)
            .max_decoding_message_size(10 * 1024 * 1024)
            .max_encoding_message_size(10 * 1024 * 1024)
    }

    /// Serve plaintext on a caller-chosen address. Use a TLS proxy or compose
    /// `into_server()` with a TLS-configured tonic Server for public deployments.
    pub async fn serve_with_shutdown(
        self,
        addr: std::net::SocketAddr,
        shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let reflection = if self.options.enable_reflection.unwrap_or(false) {
            Some(
                tonic_reflection::server::Builder::configure()
                    .register_encoded_file_descriptor_set(proto::FILE_DESCRIPTOR_SET)
                    .build_v1()?,
            )
        } else {
            None
        };
        tonic::transport::Server::builder()
            .add_service(self.into_server())
            .add_optional_service(reflection)
            .serve_with_shutdown(addr, shutdown)
            .await?;
        Ok(())
    }
}

#[tonic::async_trait]
impl<R, P> proto::dog_transport_server::DogTransport for DogGrpcService<R, P>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    async fn call(
        &self,
        request: Request<proto::CallRequest>,
    ) -> Result<Response<proto::CallResponse>, Status> {
        let authorization = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut req: DogRequest = serde_json::from_slice(&request.get_ref().request_json)
            .map_err(|_| Status::invalid_argument("Invalid DogRequest JSON"))?;
        req.transport = DogTransportKind::Grpc;
        if let Some(token) = authorization {
            let headers = req
                .params
                .inner
                .entry("headers".into())
                .or_insert_with(|| serde_json::json!({}));
            let headers = headers
                .as_object_mut()
                .ok_or_else(|| Status::invalid_argument("headers must be an object"))?;
            headers.insert("authorization".into(), token.into());
        }
        let response = crate::dispatch(&self.app, req, self.options.request_timeout_secs)
            .await
            .map_err(status)?;
        Ok(Response::new(proto::CallResponse {
            response_json: serde_json::to_vec(&response)
                .map_err(|_| Status::internal("Could not encode response"))?,
        }))
    }
}

fn status(err: DogError) -> Status {
    let safe = err.sanitize_for_client();
    let code = match safe.code() {
        400 | 411 | 422 => tonic::Code::InvalidArgument,
        401 => tonic::Code::Unauthenticated,
        403 => tonic::Code::PermissionDenied,
        404 => tonic::Code::NotFound,
        405 | 501 => tonic::Code::Unimplemented,
        408 => tonic::Code::DeadlineExceeded,
        409 => tonic::Code::AlreadyExists,
        429 => tonic::Code::ResourceExhausted,
        503 => tonic::Code::Unavailable,
        _ => tonic::Code::Internal,
    };
    Status::new(code, safe.message)
}
