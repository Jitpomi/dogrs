// src/blob_payload.rs

#![cfg(feature = "iroh")]

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BlobRefPayload {
    pub hash: String,
    pub size: u64,
}

pub struct BlobPayloadAdapter;

impl BlobPayloadAdapter {
    pub async fn put_payload<T: Serialize>(
        client: &iroh_blobs::api::blobs::Blobs,
        payload: &T,
    ) -> anyhow::Result<BlobRefPayload> {
        let serialized = serde_json::to_vec(payload)?;
        let size = serialized.len() as u64;
        anyhow::ensure!(size <= 10 * 1024 * 1024, "Blob payload exceeds 10 MiB");

        let outcome = client.add_bytes(serialized).await?;

        Ok(BlobRefPayload {
            hash: outcome.hash.to_string(),
            size,
        })
    }

    pub async fn get_payload<T: DeserializeOwned>(
        client: &iroh_blobs::api::blobs::Blobs,
        reference: &BlobRefPayload,
    ) -> anyhow::Result<T> {
        let hash: iroh_blobs::Hash = reference.hash.parse()?;

        anyhow::ensure!(
            reference.size <= 10 * 1024 * 1024,
            "Blob payload exceeds 10 MiB"
        );
        match client.status(hash).await? {
            iroh_blobs::api::proto::BlobStatus::Complete { size } => {
                anyhow::ensure!(
                    size == reference.size && size <= 10 * 1024 * 1024,
                    "Blob payload size mismatch"
                );
            }
            _ => anyhow::bail!("Blob payload is not locally complete"),
        }
        let bytes = client.get_bytes(hash).await?;

        let deserialized = serde_json::from_slice(&bytes)?;
        Ok(deserialized)
    }
}
