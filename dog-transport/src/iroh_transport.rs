// src/iroh_transport.rs

use crate::{IntoDogService, IrohOptions};
use dog_core::{DogApp, DogRequest, DogResponse, DogTransportKind};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use std::collections::HashMap;
use std::str::FromStr;

/// Framed reader: reads 4-byte big-endian length prefix followed by body bytes.
async fn read_frame<R: tokio::io::AsyncRead + Unpin>(mut reader: R) -> anyhow::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes).await?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    anyhow::ensure!(len <= 10 * 1024 * 1024, "Iroh request exceeds 10 MiB");
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Framed writer: writes 4-byte big-endian length prefix followed by body bytes.
async fn write_frame<W: tokio::io::AsyncWrite + Unpin>(
    mut writer: W,
    data: &[u8],
) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    anyhow::ensure!(
        data.len() <= 10 * 1024 * 1024,
        "Iroh response exceeds 10 MiB"
    );
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
    streams: std::sync::Arc<tokio::sync::Semaphore>,
}

impl<R, P> DogIrohService<R, P>
where
    R: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
    P: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
{
    pub fn new(app: DogApp<R, P>) -> Self {
        Self {
            app,
            streams: std::sync::Arc::new(tokio::sync::Semaphore::new(128)),
        }
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

        // Bound active requests across all peers; keep tasks owned by their connection.
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            let stream = tokio::select! {
                stream = connection.accept_bi() => stream,
                _ = tasks.join_next(), if !tasks.is_empty() => continue,
            };
            let Ok((mut send, mut recv)) = stream else {
                break;
            };
            let Ok(permit) = self.streams.clone().try_acquire_owned() else {
                // Dropping the streams resets this request without allocating a body.
                continue;
            };
            let app = app.clone();

            tasks.spawn(async move {
                let _permit = permit;
                // Read query frame
                let req_bytes = match tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    read_frame(&mut recv),
                )
                .await
                {
                    Ok(Ok(b)) => b,
                    error => {
                        tracing::error!("Error reading request frame: {:?}", error);
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
                dog_request.metadata.insert(
                    "peer_id".to_string(),
                    serde_json::Value::String(peer_id.to_string()),
                );

                // Handle the request via DogApp
                let dog_response = match app.handle(dog_request).await {
                    Ok(res) => res,
                    Err(err) => DogResponse {
                        payload: None,
                        metadata: {
                            let mut map = HashMap::new();
                            map.insert(
                                "error".to_string(),
                                serde_json::Value::String(err.sanitize_for_client().message),
                            );
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

                match tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    write_frame(&mut send, &res_bytes),
                )
                .await
                {
                    Ok(Ok(())) => {
                        let _ = send.finish();
                    }
                    error => tracing::error!(?error, "Error writing response frame"),
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
    type Service =
        std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<Router>> + Send>>;

    fn into_service(self, options: IrohOptions) -> Self::Service {
        Box::pin(async move {
            let endpoint = match options.endpoint {
                Some(ep) => ep,
                None => {
                    let mut builder = iroh::Endpoint::builder(iroh::endpoint::presets::N0);
                    if let Some(key) = options.secret_key {
                        builder = builder.secret_key(iroh::SecretKey::from_str(&key)?);
                    }
                    if let Some(url) = options.relay_url {
                        builder = builder.relay_mode(if url == "disabled" {
                            iroh::endpoint::RelayMode::Disabled
                        } else {
                            iroh::endpoint::RelayMode::Custom(iroh::RelayMap::from_iter([
                                url.parse::<iroh::RelayUrl>()?,
                            ]))
                        });
                    }
                    builder.bind().await?
                }
            };
            Ok(Router::builder(endpoint)
                .accept(options.alpn, DogIrohService::new(self))
                .spawn())
        })
    }
}

impl<R, P> IntoDogService<(iroh::protocol::RouterBuilder, Vec<u8>)> for DogApp<R, P>
where
    R: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
    P: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
{
    type Service = iroh::protocol::RouterBuilder;

    fn into_service(
        self,
        (builder, alpn): (iroh::protocol::RouterBuilder, Vec<u8>),
    ) -> Self::Service {
        let service = DogIrohService::new(self);
        builder.accept(alpn, service)
    }
}
