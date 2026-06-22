// tests/blob_payload_test.rs

#![cfg(feature = "iroh")]

use dog_transport::BlobPayloadAdapter;
use iroh_blobs::store::mem::MemStore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct DummyPayload {
    name: String,
    data: Vec<u8>,
}

#[tokio::test]
async fn test_iroh_blobs_adapter_lifecycle() -> anyhow::Result<()> {
    // Initialize in-memory store
    let store = MemStore::new();
    
    // Create a payload to upload
    let payload = DummyPayload {
        name: "dogrs-job-payload".to_string(),
        data: vec![1, 2, 3, 4, 5, 42],
    };
    
    // Store it
    let ref_payload = BlobPayloadAdapter::put_payload(&store, &payload)
        .await?;
        
    assert!(!ref_payload.hash.is_empty());
    assert!(ref_payload.size > 0);
    
    // Get it back
    let retrieved: DummyPayload = BlobPayloadAdapter::get_payload(&store, &ref_payload)
        .await?;
        
    assert_eq!(payload, retrieved);
    Ok(())
}
