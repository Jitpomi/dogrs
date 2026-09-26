// tests/iroh_test.rs

#![cfg(feature = "iroh")]

use async_trait::async_trait;
use dog_core::{DogApp, DogMethod, DogParams, DogRequest, DogResponse, DogService, TenantContext};
use dog_transport::{IntoDogService, IrohOptions};
use iroh::{endpoint::presets, Endpoint};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct TestData {
    id: Option<String>,
    message: String,
}

struct TestService;

#[async_trait]
impl DogService<TestData, ()> for TestService {
    async fn get(&self, _ctx: &TenantContext, id: &str, _params: ()) -> anyhow::Result<TestData> {
        Ok(TestData {
            id: Some(id.to_string()),
            message: "Hello from Iroh!".to_string(),
        })
    }

    async fn find(&self, _ctx: &TenantContext, _params: ()) -> anyhow::Result<Vec<TestData>> {
        Ok(vec![TestData {
            id: None,
            message: "Hello from Iroh Find!".to_string(),
        }])
    }
}

/// Helper to read framed bytes (prefixed by 4-byte big-endian length)
async fn read_frame<R: tokio::io::AsyncRead + Unpin>(mut reader: R) -> anyhow::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes).await?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Helper to write framed bytes (prefixed by 4-byte big-endian length)
async fn write_frame<W: tokio::io::AsyncWrite + Unpin>(
    mut writer: W,
    data: &[u8],
) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    let len = data.len() as u32;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(data).await?;
    writer.flush().await?;
    Ok(())
}

#[tokio::test]
async fn test_iroh_transport_lifecycle() -> anyhow::Result<()> {
    // 1. Build DogApp and register service
    let mut builder = DogApp::builder();
    builder.register_service("test_service", Arc::new(TestService));
    let app = builder.build();

    // 2. Start Server Router
    let alpn = b"dogrs/test/echo/0".to_vec();
    let options = IrohOptions::new(alpn.clone()).relay_url("disabled");
    let router = app.into_service(options).await?;

    // Get server address (both NodeId and local socket addrs)
    let server_addr = loopback_addr(router.endpoint());

    // 3. Connect client
    let client_ep = Endpoint::builder(presets::N0)
        .relay_mode(iroh::endpoint::RelayMode::Disabled)
        .bind()
        .await?;
    let conn = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client_ep.connect(server_addr, &alpn),
    )
    .await??;
    let (mut send, mut recv) = conn.open_bi().await?;

    // 4. Send DogRequest over bidirectional stream
    let req = DogRequest {
        request_id: Some("tx-123".to_string()),
        transport: dog_core::DogTransportKind::Custom("iroh-test".to_string()),
        service: "test_service".to_string(),
        method: DogMethod::Get,
        id: Some("item-456".to_string()),
        tenant: TenantContext::new("my-tenant"),
        params: DogParams::from(HashMap::new()),
        payload: None,
        metadata: HashMap::new(),
    };

    let req_bytes = serde_json::to_vec(&req)?;
    write_frame(&mut send, &req_bytes).await?;
    send.finish()?;

    // 5. Read response
    let res_bytes = read_frame(&mut recv).await?;
    let res: DogResponse = serde_json::from_slice(&res_bytes)?;

    // Verify response
    assert!(res.payload.is_some());
    let data: TestData = serde_json::from_value(res.payload.unwrap())?;
    assert_eq!(data.id, Some("item-456".to_string()));
    assert_eq!(data.message, "Hello from Iroh!");

    // 6. Graceful shutdown
    conn.close(0u32.into(), b"done");
    client_ep.close().await;
    router.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_iroh_transport_shared_endpoint() -> anyhow::Result<()> {
    // 1. Build DogApp and register service
    let mut builder = DogApp::builder();
    builder.register_service("test_service", Arc::new(TestService));
    let app = builder.build();

    // 2. Create the Endpoint manually
    let endpoint = Endpoint::builder(presets::N0)
        .relay_mode(iroh::endpoint::RelayMode::Disabled)
        .bind()
        .await?;

    // 3. Start Server Router with the pre-existing Endpoint
    let alpn = b"dogrs/test/echo/1".to_vec();
    let options = IrohOptions::new(alpn.clone()).endpoint(endpoint.clone());
    let router = app.into_service(options).await?;

    // Get server address (both NodeId and local socket addrs)
    let server_addr = loopback_addr(router.endpoint());

    // 4. Connect client
    let client_ep = Endpoint::builder(presets::N0)
        .relay_mode(iroh::endpoint::RelayMode::Disabled)
        .bind()
        .await?;
    let conn = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client_ep.connect(server_addr, &alpn),
    )
    .await??;
    let (mut send, mut recv) = conn.open_bi().await?;

    // 5. Send DogRequest over bidirectional stream
    let req = DogRequest {
        request_id: Some("tx-789".to_string()),
        transport: dog_core::DogTransportKind::Custom("iroh-test-shared".to_string()),
        service: "test_service".to_string(),
        method: DogMethod::Get,
        id: Some("item-999".to_string()),
        tenant: TenantContext::new("my-tenant"),
        params: DogParams::from(HashMap::new()),
        payload: None,
        metadata: HashMap::new(),
    };

    let req_bytes = serde_json::to_vec(&req)?;
    write_frame(&mut send, &req_bytes).await?;
    send.finish()?;

    // 6. Read response
    let res_bytes = read_frame(&mut recv).await?;
    let res: DogResponse = serde_json::from_slice(&res_bytes)?;

    // Verify response
    assert!(res.payload.is_some());
    let data: TestData = serde_json::from_value(res.payload.unwrap())?;
    assert_eq!(data.id, Some("item-999".to_string()));
    assert_eq!(data.message, "Hello from Iroh!");

    // 7. Graceful shutdown
    conn.close(0u32.into(), b"done");
    client_ep.close().await;
    router.shutdown().await?;

    Ok(())
}

#[derive(Debug, Clone)]
struct MockGossipHandler;

impl iroh::protocol::ProtocolHandler for MockGossipHandler {
    async fn accept(
        &self,
        _connection: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {
        Ok(())
    }
}

#[tokio::test]
async fn test_iroh_transport_builder_composition() -> anyhow::Result<()> {
    // 1. Build DogApp and register service
    let mut builder = DogApp::builder();
    builder.register_service("test_service", Arc::new(TestService));
    let app = builder.build();

    // 2. Create the Endpoint manually
    let endpoint = Endpoint::builder(presets::N0)
        .relay_mode(iroh::endpoint::RelayMode::Disabled)
        .bind()
        .await?;

    // 3. Start building the Router
    let router_builder = iroh::protocol::Router::builder(endpoint);

    // 4. Use the IntoDogService implementation to register DogRS on the router builder
    let alpn = b"dogrs/test/echo/2".to_vec();
    let router_builder = app.into_service((router_builder, alpn.clone()));

    // 5. Register a completely independent custom protocol on the same router builder!
    let router_builder = router_builder.accept(b"iroh-gossip/test/0".to_vec(), MockGossipHandler);

    // 6. Spawn the composed router
    let router = router_builder.spawn();

    // 7. Verify we can still perform DogRS RPC on the composed router
    let server_addr = loopback_addr(router.endpoint());
    let client_ep = Endpoint::builder(presets::N0)
        .relay_mode(iroh::endpoint::RelayMode::Disabled)
        .bind()
        .await?;
    let conn = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client_ep.connect(server_addr, &alpn),
    )
    .await??;
    let (mut send, mut recv) = conn.open_bi().await?;

    let req = DogRequest {
        request_id: Some("tx-abc".to_string()),
        transport: dog_core::DogTransportKind::Custom("iroh-test-composed".to_string()),
        service: "test_service".to_string(),
        method: DogMethod::Get,
        id: Some("item-111".to_string()),
        tenant: TenantContext::new("my-tenant"),
        params: DogParams::from(HashMap::new()),
        payload: None,
        metadata: HashMap::new(),
    };

    let req_bytes = serde_json::to_vec(&req)?;
    write_frame(&mut send, &req_bytes).await?;
    send.finish()?;

    let res_bytes = read_frame(&mut recv).await?;
    let res: DogResponse = serde_json::from_slice(&res_bytes)?;

    assert!(res.payload.is_some());
    let data: TestData = serde_json::from_value(res.payload.unwrap())?;
    assert_eq!(data.id, Some("item-111".to_string()));

    // 8. Graceful shutdown
    conn.close(0u32.into(), b"done");
    client_ep.close().await;
    router.shutdown().await?;

    Ok(())
}

fn loopback_addr(endpoint: &Endpoint) -> iroh::EndpointAddr {
    let mut socket = endpoint
        .bound_sockets()
        .into_iter()
        .find(|s| s.is_ipv4())
        .expect("IPv4 socket");
    socket.set_ip(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    iroh::EndpointAddr::new(endpoint.id()).with_ip_addr(socket)
}
