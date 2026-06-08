use crate::{codec::JobCodec, QueueError, QueueResult};

/// JSON codec for job serialization.
///
/// Both `encode_bytes` and `decode_bytes` validate that the bytes are
/// well-formed JSON before accepting them. This catches payload corruption
/// and programming errors (e.g. passing raw binary with `codec="json"`) at
/// the enqueue/dequeue boundary rather than surfacing a confusing
/// deserialization error deep inside a job handler.
#[derive(Debug, Clone)]
pub struct JsonCodec;

impl JobCodec for JsonCodec {
    fn encode_bytes(&self, bytes: &[u8]) -> QueueResult<Vec<u8>> {
        // Validates well-formedness before storing. This guard is meaningful for
        // direct callers of `encode_bytes` (i.e. custom backend implementations).
        //
        // When called via `CodecRegistry::encode_job`, the input bytes are always
        // from `serde_json::to_vec(job)` which already guarantees valid JSON, so
        // this scan is redundant in that specific hot path. The trade-off is
        // acceptable: encode_bytes is a public API and must defend against direct
        // callers. `IgnoredAny` short-circuits after structure validation without
        // allocating a Value AST, keeping the overhead minimal.
        serde_json::from_slice::<serde::de::IgnoredAny>(bytes).map_err(|e| {
            QueueError::SerializationError(format!("Payload is not valid JSON: {e}"))
        })?;
        Ok(bytes.to_vec())
    }

    fn decode_bytes(&self, bytes: &[u8]) -> QueueResult<Vec<u8>> {
        // Validate well-formedness before returning to caller.
        // `IgnoredAny` validates structure without a Value allocation (see encode_bytes).
        serde_json::from_slice::<serde::de::IgnoredAny>(bytes).map_err(|e| {
            QueueError::SerializationError(format!("Stored payload is corrupted (not valid JSON): {e}"))
        })?;
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
        let codec = JsonCodec;
        let job = TestJob {
            id: 42,
            name: "test job".to_string(),
        };

        // Encode via serde_json, pass through the codec validation
        let bytes = serde_json::to_vec(&job).unwrap();
        let encoded = codec.encode_bytes(&bytes).unwrap();
        assert_eq!(encoded, bytes);

        // Decode and deserialise back to the original type
        let decoded_bytes = codec.decode_bytes(&encoded).unwrap();
        let decoded: TestJob = serde_json::from_slice(&decoded_bytes).unwrap();
        assert_eq!(job, decoded);
    }

    #[test]
    fn test_encode_rejects_non_json() {
        let codec = JsonCodec;
        let garbage = b"\xff\xfe binary garbage \x00";
        assert!(
            codec.encode_bytes(garbage).is_err(),
            "encode_bytes must reject non-JSON bytes"
        );
    }

    #[test]
    fn test_decode_rejects_corrupted_payload() {
        let codec = JsonCodec;
        let truncated = b"{\"id\": 42, \"name\":";
        assert!(
            codec.decode_bytes(truncated).is_err(),
            "decode_bytes must reject truncated/corrupted JSON"
        );
    }

    #[test]
    fn test_codec_id() {
        let codec = JsonCodec;
        assert_eq!(codec.codec_id(), "json");
    }
}
