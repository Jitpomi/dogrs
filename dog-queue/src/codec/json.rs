use crate::{QueueResult, codec::JobCodec};

/// JSON codec for job serialization
#[derive(Debug, Clone)]
pub struct JsonCodec;

impl JobCodec for JsonCodec {
    fn encode_bytes(&self, bytes: &[u8]) -> QueueResult<Vec<u8>> {
        // For JSON codec, we assume the input is already JSON bytes
        Ok(bytes.to_vec())
    }

    fn decode_bytes(&self, bytes: &[u8]) -> QueueResult<Vec<u8>> {
        // For JSON codec, we assume the output should be JSON bytes
        Ok(bytes.to_vec())
    }

    fn codec_id(&self) -> &'static str {
        "json"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestJob {
        id: u32,
        name: String,
    }

    #[test]
    fn test_json_codec_roundtrip() {
        let _codec = JsonCodec;
        let job = TestJob {
            id: 42,
            name: "test job".to_string(),
        };

        // Encode
        let bytes = serde_json::to_vec(&job).unwrap();
        assert!(!bytes.is_empty());

        // Decode
        let decoded: TestJob = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(job, decoded);
    }

    #[test]
    fn test_codec_id() {
        let codec = JsonCodec;
        assert_eq!(codec.codec_id(), "json");
    }
}
