// src/iroh_transport.rs

use std::collections::HashMap;
use std::str::FromStr;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use dog_core::{DogApp, DogRequest, DogResponse, DogTransportKind};
use crate::{IntoDogService, IrohOptions};

/// Framed reader: reads 4-byte big-endian length prefix followed by body bytes.
async fn read_frame<R: tokio::io::AsyncRead + Unpin>(mut reader: R) -> anyhow::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes).await?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Framed writer: writes 4-byte big-endian length prefix followed by body bytes.
async fn write_frame<W: tokio::io::AsyncWrite + Unpin>(mut writer: W, data: &[u8]) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    let len = data.len() as u32;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(data).await?;
    writer.flush().await?;
    Ok(())
}

/// A protocol handler that handles incoming QUIC connections from remote iroh peers
/// and dispatches requests to the underlying `DogApp` service registry.
pub struct DogIrohService<R, P>
where
    R: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
    P: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
{
    app: DogApp<R, P>,
}

impl<R, P> DogIrohService<R, P>
where
    R: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
    P: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
{
    pub fn new(app: DogApp<R, P>) -> Self {
        Self { app }
    }
}

impl<R, P> std::fmt::Debug for DogIrohService<R, P>
where
    R: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
    P: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DogIrohService").finish()
    }
}

impl<R, P> ProtocolHandler for DogIrohService<R, P>
where
    R: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
    P: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
{
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let peer_id = connection.remote_id();
        tracing::info!("Accepted P2P connection from peer: {}", peer_id);

        let app = self.app.clone();

        // Keep accepting bidirectional streams on the same QUIC connection
        while let Ok((mut send, mut recv)) = connection.accept_bi().await {
            let app = app.clone();
            let peer_id = peer_id.clone();

            tokio::spawn(async move {
                // Read query frame
                let req_bytes = match read_frame(&mut recv).await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::error!("Error reading request frame: {}", e);
                        return;
                    }
                };

                // Deserialize query
                let mut dog_request: DogRequest = match serde_json::from_slice(&req_bytes) {
                    Ok(req) => req,
                    Err(e) => {
                        tracing::error!("Failed to deserialize request: {}", e);
                        return;
                    }
                };

                // Set transport and origin metadata
                dog_request.transport = DogTransportKind::Custom("iroh-p2p".to_string());
                dog_request.metadata.insert("peer_id".to_string(), serde_json::Value::String(peer_id.to_string()));

                // Handle the request via DogApp
                let dog_response = match app.handle(dog_request).await {
                    Ok(res) => res,
                    Err(err) => DogResponse {
                        payload: None,
                        metadata: {
                            let mut map = HashMap::new();
                            map.insert("error".to_string(), serde_json::Value::String(err.message));
                            map
                        },
                    },
                };

                // Serialize and respond
                let res_bytes = match serde_json::to_vec(&dog_response) {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::error!("Failed to serialize response: {}", e);
                        return;
                    }
                };

                if let Err(e) = write_frame(&mut send, &res_bytes).await {
                    tracing::error!("Error writing response frame: {}", e);
                } else {
                    let _ = send.finish();
                }
            });
        }

        Ok(())
    }
}

impl<R, P> IntoDogService<IrohOptions> for DogApp<R, P>
where
    R: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
    P: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
{
    type Service = Router;

    fn into_service(self, options: IrohOptions) -> Self::Service {
        let endpoint = match options.endpoint {
            Some(ep) => ep,
            None => {
                // 1. Configure the Endpoint (persistent or ephemeral keypair)
                let mut builder = iroh::Endpoint::builder(iroh::endpoint::presets::N0);
                if let Some(key_str) = options.secret_key {
                    let secret = iroh::SecretKey::from_str(&key_str).unwrap();
                    builder = builder.secret_key(secret);
                }
                
                if let Some(ref r_url) = options.relay_url {
                    if r_url == "disabled" {
                        builder = builder.relay_mode(iroh::endpoint::RelayMode::Disabled);
                    }
                }
                
                futures::executor::block_on(builder.bind()).unwrap()
            }
        };
        
        let service = DogIrohService::new(self);
        
        // 2. Spawn the P2P protocol router
        Router::builder(endpoint)
            .accept(options.alpn, service)
            .spawn()
    }
}

impl<R, P> IntoDogService<(iroh::protocol::RouterBuilder, Vec<u8>)> for DogApp<R, P>
where
    R: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
    P: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
{
    type Service = iroh::protocol::RouterBuilder;

    fn into_service(self, (builder, alpn): (iroh::protocol::RouterBuilder, Vec<u8>)) -> Self::Service {
        let service = DogIrohService::new(self);
        builder.accept(alpn, service)
    }
}
