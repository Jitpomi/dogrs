use dog_auth::AuthParams;
use dog_core::{
    DogApp, DogAppBuilder, DogMethod, DogParams, DogRequest, DogService, DogTransportKind,
    TenantContext,
};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
type Params = AuthParams<Value>;

struct Echo;
#[async_trait::async_trait]
impl DogService<Value, Params> for Echo {
    async fn create(
        &self,
        _: &TenantContext,
        data: Value,
        params: Params,
    ) -> anyhow::Result<Value> {
        Ok(
            json!({"data": data, "provider": params.provider, "authenticated": params.authenticated}),
        )
    }
}
fn app() -> DogApp<Value, Params> {
    let mut app = DogAppBuilder::new();
    app.register_service("echo", Arc::new(Echo));
    app.build()
}
#[cfg(any(feature = "grpc", feature = "cli"))]
fn request() -> DogRequest {
    DogRequest {
        request_id: Some("test-request".into()),
        transport: DogTransportKind::Internal, // The transport must override this.
        service: "echo".into(),
        method: DogMethod::Create,
        id: None,
        tenant: TenantContext::new("test"),
        params: DogParams::from(HashMap::from([
            ("inner".into(), Value::Null),
            ("headers".into(), json!({})),
            ("authenticated".into(), json!(true)),
            ("provider".into(), Value::Null),
        ])),
        payload: Some(json!({"hello": "world"})),
        metadata: HashMap::new(),
    }
}

#[cfg(feature = "cli")]
#[tokio::test]
async fn cli_recovers_from_invalid_input_and_rejects_trusted_flags() {
    use dog_transport::{CliOptions, IntoDogService};
    let cli = app().into_service(CliOptions::new());
    let input = format!("not-json\n{}\n", serde_json::to_string(&request()).unwrap());
    let mut output = Vec::new();
    cli.run(input.as_bytes(), &mut output).await.unwrap();
    let lines: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(lines[0]["error"]["code"], 400);
    assert_eq!(lines[1]["payload"]["data"]["hello"], "world");
    assert_eq!(lines[1]["payload"]["provider"], "cli");
    assert_eq!(lines[1]["payload"]["authenticated"], false);
}

#[cfg(feature = "http")]
#[tokio::test]
async fn http_supports_auth_params_and_limits_streamed_bodies() {
    use bytes::Bytes;
    use dog_transport::{HttpOptions, IntoDogService};
    use http_body_util::{BodyExt, Full};
    use tower::Service;
    let mut http = app().into_service(HttpOptions::new().body_limit(32));
    let req = http::Request::post("/echo")
        .body(Full::new(Bytes::from_static(b"{\"hello\":\"world\"}")))
        .unwrap();
    let response = http.call(req).await.unwrap();
    assert_eq!(response.status(), 200);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["authenticated"], false);
    assert_eq!(body["provider"], "rest");
    let req = http::Request::post("/echo")
        .body(Full::new(Bytes::from(vec![b'x'; 33])))
        .unwrap();
    assert!(http.call(req).await.unwrap().status().is_client_error());
}

#[cfg(feature = "grpc")]
#[tokio::test]
async fn grpc_roundtrip_and_error_mapping() {
    use dog_transport::{
        grpc::proto::{dog_transport_client::DogTransportClient, CallRequest},
        GrpcOptions, IntoDogService,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(app().into_service(GrpcOptions::new()).into_server())
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async {
                    let _ = stopped.await;
                },
            ),
    );
    let mut client = DogTransportClient::connect(format!("http://{addr}"))
        .await
        .unwrap();
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client.call(CallRequest {
            request_json: serde_json::to_vec(&request()).unwrap(),
        }),
    )
    .await
    .unwrap()
    .unwrap()
    .into_inner();
    let value: Value = serde_json::from_slice(&response.response_json).unwrap();
    assert_eq!(value["payload"]["data"]["hello"], "world");
    assert_eq!(value["payload"]["authenticated"], false);
    assert_eq!(value["payload"]["provider"], "grpc");
    let err = client
        .call(CallRequest {
            request_json: b"bad-json".to_vec(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    let mut missing = request();
    missing.service = "missing".into();
    let err = client
        .call(CallRequest {
            request_json: serde_json::to_vec(&missing).unwrap(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[cfg(feature = "cli")]
#[tokio::test]
async fn transport_cannot_bypass_authentication_with_internal_or_authenticated_flags() {
    use dog_auth::{AuthOptions, AuthenticateHook, AuthenticationService, JwtStrategy};
    use dog_transport::{CliOptions, IntoDogService};
    let mut builder = DogAppBuilder::<Value, Params>::new();
    let mut options = AuthOptions::default();
    options.jwt.secret = Some("test-key-at-least-32-bytes-long-for-audit".into());
    let mut auth_builder = AuthenticationService::builder(&mut builder, Some(options)).unwrap();
    auth_builder.register("jwt", Arc::new(JwtStrategy::new()));
    let auth = Arc::new(AuthenticationService::new(Arc::new(auth_builder.build())));
    let adapter = AuthenticationService::install(&mut builder, auth.clone());
    builder.register_service("echo", Arc::new(Echo));
    builder.service_hooks("echo", |h| {
        h.before_all(Arc::new(AuthenticateHook::new(
            auth.clone(),
            vec!["jwt".into()],
        )));
    });
    let app = builder.build();
    adapter.setup(app.clone());
    let cli = app.into_service(CliOptions::new());
    let forged = request();
    let mut output = Vec::new();
    cli.run(
        format!("{}\n", serde_json::to_string(&forged).unwrap()).as_bytes(),
        &mut output,
    )
    .await
    .unwrap();
    let result: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(result["error"]["code"], 401);
    let token = auth
        .base
        .create_access_token(json!({"sub":"test-user"}), None)
        .await
        .unwrap();
    let mut valid = request();
    valid.params.inner.insert(
        "headers".into(),
        json!({"authorization":format!("Bearer {token}")}),
    );
    let mut output = Vec::new();
    cli.run(
        format!("{}\n", serde_json::to_string(&valid).unwrap()).as_bytes(),
        &mut output,
    )
    .await
    .unwrap();
    let result: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(result["payload"]["authenticated"], true);
}
