// src/blob_payload.rs

#![cfg(feature = "iroh")]

use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;

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
            
        let bytes = client.get_bytes(hash).await?;
            
        let deserialized = serde_json::from_slice(&bytes)?;
        Ok(deserialized)
    }
}
