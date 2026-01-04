use base64::Engine;

/// Utility for decoding MIME-encoded strings (RFC 2047 format)
pub struct MimeDecoder;

impl MimeDecoder {
    /// Decode MIME-encoded string like =?UTF-8?B?base64data?=
    pub fn decode(input: &str) -> String {
        if !input.starts_with("=?") || !input.ends_with("?=") {
            return input.to_string();
        }

        // Handle single encoded segment
        if let Some(decoded) = Self::decode_segment(input) {
            return decoded;
        }

        // Handle multiple encoded segments (split by space)
        let segments: Vec<&str> = input.split_whitespace().collect();
        if segments.len() > 1 {
            let decoded_parts: Vec<String> = segments
                .iter()
                .filter_map(|segment| Self::decode_segment(segment))
                .collect();
            
            if !decoded_parts.is_empty() {
                return decoded_parts.join("");
            }
        }

        // Return original if decoding fails
        input.to_string()
    }

    /// Decode a single MIME segment
    fn decode_segment(segment: &str) -> Option<String> {
        let content = segment.strip_prefix("=?UTF-8?B?")?.strip_suffix("?=")?;
        let decoded_bytes = base64::engine::general_purpose::STANDARD.decode(content).ok()?;
        String::from_utf8(decoded_bytes).ok()
    }

    /// Decode optional string field
    pub fn decode_option(input: Option<&String>) -> Option<String> {
        input.map(|s| Self::decode(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_simple() {
        let encoded = "=?UTF-8?B?SGVsbG8gV29ybGQ=?="; // "Hello World"
        assert_eq!(MimeDecoder::decode(encoded), "Hello World");
    }

    #[test]
    fn test_decode_non_encoded() {
        let plain = "Hello World";
        assert_eq!(MimeDecoder::decode(plain), "Hello World");
    }

    #[test]
    fn test_decode_multiple_segments() {
        let encoded = "=?UTF-8?B?SGVsbG8=?= =?UTF-8?B?IFdvcmxk?="; // "Hello" + " World"
        assert_eq!(MimeDecoder::decode(encoded), "Hello World");
    }
}
