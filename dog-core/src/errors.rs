//! # Errors (Feathers-style)
//!
//! DogRS provides a Feathers-inspired set of structured errors.
//! Core goals:
//! - consistent status codes + class names
//! - can be carried through anyhow::Error (for hook pipeline)
//! - transport-agnostic (server crate decides how to serialize)
//!
//! ### Dynamic Metadata (Data & Errors)
//! If you enable feature `json` (on by default):
//! - `data` / `errors` are typed as `serde_json::Value`
//! 
//! If you use `default-features = false`:
//! - `data` / `errors` fallback to the format-agnostic `DogValue` enum,
//!   allowing `dog-core` to be used without the `serde_json` dependency.

use std::fmt;

use anyhow::Error as AnyError;

/// A convenience result type for DogRS core APIs.
pub type DogResult<T> = std::result::Result<T, AnyError>;

/// Feathers-ish error class names + status codes.
///
/// # Extensibility
/// This enum is `#[non_exhaustive]` — new variants may be added in minor releases.
/// Match arms against `ErrorKind` must include a catch-all `_` arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    BadRequest,         // 400
    NotAuthenticated,   // 401
    Forbidden,          // 403
    NotFound,           // 404
    MethodNotAllowed,   // 405
    NotAcceptable,      // 406
    Timeout,            // 408
    Conflict,           // 409
    Gone,               // 410
    LengthRequired,     // 411
    Unprocessable,      // 422
    TooManyRequests,    // 429
    GeneralError,       // 500
    NotImplemented,     // 501
    BadGateway,         // 502
    Unavailable,        // 503
}

impl ErrorKind {
    pub fn status_code(&self) -> u16 {
        match self {
            ErrorKind::BadRequest => 400,
            ErrorKind::NotAuthenticated => 401,
            ErrorKind::Forbidden => 403,
            ErrorKind::NotFound => 404,
            ErrorKind::MethodNotAllowed => 405,
            ErrorKind::NotAcceptable => 406,
            ErrorKind::Timeout => 408,
            ErrorKind::Conflict => 409,
            ErrorKind::Gone => 410,
            ErrorKind::LengthRequired => 411,
            ErrorKind::Unprocessable => 422,
            ErrorKind::TooManyRequests => 429,
            ErrorKind::GeneralError => 500,
            ErrorKind::NotImplemented => 501,
            ErrorKind::BadGateway => 502,
            ErrorKind::Unavailable => 503,
        }
    }

    /// Feathers error `name` (e.g. "NotFound")
    pub fn name(&self) -> &'static str {
        match self {
            ErrorKind::BadRequest => "BadRequest",
            ErrorKind::NotAuthenticated => "NotAuthenticated",
            ErrorKind::Forbidden => "Forbidden",
            ErrorKind::NotFound => "NotFound",
            ErrorKind::MethodNotAllowed => "MethodNotAllowed",
            ErrorKind::NotAcceptable => "NotAcceptable",
            ErrorKind::Timeout => "Timeout",
            ErrorKind::Conflict => "Conflict",
            ErrorKind::Gone => "Gone",
            ErrorKind::LengthRequired => "LengthRequired",
            ErrorKind::Unprocessable => "Unprocessable",
            ErrorKind::TooManyRequests => "TooManyRequests",
            ErrorKind::GeneralError => "GeneralError",
            ErrorKind::NotImplemented => "NotImplemented",
            ErrorKind::BadGateway => "BadGateway",
            ErrorKind::Unavailable => "Unavailable",
        }
    }

    /// Feathers error `className` (commonly kebab-cased)
    pub fn class_name(&self) -> &'static str {
        match self {
            ErrorKind::BadRequest => "bad-request",
            ErrorKind::NotAuthenticated => "not-authenticated",
            ErrorKind::Forbidden => "forbidden",
            ErrorKind::NotFound => "not-found",
            ErrorKind::MethodNotAllowed => "method-not-allowed",
            ErrorKind::NotAcceptable => "not-acceptable",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Conflict => "conflict",
            ErrorKind::Gone => "gone",
            ErrorKind::LengthRequired => "length-required",
            ErrorKind::Unprocessable => "unprocessable",
            ErrorKind::TooManyRequests => "too-many-requests",
            ErrorKind::GeneralError => "general-error",
            ErrorKind::NotImplemented => "not-implemented",
            ErrorKind::BadGateway => "bad-gateway",
            ErrorKind::Unavailable => "unavailable",
        }
    }
}

#[cfg(feature = "json")]
pub type ErrorValue = serde_json::Value;

#[cfg(all(feature = "serde", not(feature = "json")))]
pub type ErrorValue = DogValue;

#[cfg(not(any(feature = "serde", feature = "json")))]
pub type ErrorValue = std::sync::Arc<dyn std::any::Any + Send + Sync>;

#[cfg(all(feature = "serde", not(feature = "json")))]
/// Format-agnostic dynamic value used as `ErrorValue` when `json` feature is disabled.
///
/// # Serialization
/// Works for **all** serde-supported formats including non-self-describing ones (Bincode,
/// MessagePack) via the standard serde `serialize_*` primitives. Uses `#[serde(untagged)]`
/// so JSON output is idiomatic (`true`, `42`, `"hello"`), not tagged (`{"Bool": true}`).
///
/// # Deserialization
/// Works for **self-describing** formats only:
/// - JSON ✅ — type inferred from tokens
/// - TOML ✅ — type inferred from tokens
/// - YAML ✅ — type inferred from tokens
/// - MessagePack (`rmp-serde`) ✅ — each MsgPack byte carries a type marker
/// - Bincode ❌ — Bincode's `deserialize_any` is not implemented; it requires the Rust
///   type system to specify the type before reading. Bincode is an efficient binary format
///   by design: no type metadata in the wire bytes means no dynamic dispatch.
///
/// # Precision Notes
/// - `Integer` and `Float` are distinct variants — `i64` precision is preserved for large
///   values (IDs, timestamps) that `f64` would silently corrupt above 2⁵³.
/// - `Object` uses `BTreeMap` for deterministic, alphabetically-sorted field order.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum DogValue {
    Null,
    Bool(bool),
    /// Exact 64-bit integer — use this for IDs, timestamps, counts.
    Integer(i64),
    /// Floating-point number — use this for rates, percentages, measurements.
    ///
    /// # Warning
    /// `f64::NAN` and `f64::INFINITY` are not valid in JSON or TOML and will cause
    /// a serialization error at runtime. Use [`DogValue::float`] to construct safely.
    Float(f64),
    String(String),
    Array(Vec<DogValue>),
    /// Fields are stored in `BTreeMap` — alphabetically sorted, deterministic output.
    Object(std::collections::BTreeMap<String, DogValue>),
}

#[cfg(all(feature = "serde", not(feature = "json")))]
impl DogValue {
    /// Construct a `Float` variant, returning `None` if the value is NaN or infinite.
    ///
    /// Prefer this over `DogValue::Float(v)` directly to catch invalid float values
    /// at construction time rather than at serialization time.
    pub fn float(v: f64) -> Option<Self> {
        if v.is_finite() { Some(DogValue::Float(v)) } else { None }
    }
}

#[cfg(all(feature = "serde", not(feature = "json")))]
impl From<String> for DogValue {
    fn from(s: String) -> Self { DogValue::String(s) }
}
#[cfg(all(feature = "serde", not(feature = "json")))]
impl From<&str> for DogValue {
    fn from(s: &str) -> Self { DogValue::String(s.to_owned()) }
}
#[cfg(all(feature = "serde", not(feature = "json")))]
impl From<bool> for DogValue {
    fn from(b: bool) -> Self { DogValue::Bool(b) }
}
#[cfg(all(feature = "serde", not(feature = "json")))]
impl From<i64> for DogValue {
    fn from(n: i64) -> Self { DogValue::Integer(n) }
}
#[cfg(all(feature = "serde", not(feature = "json")))]
impl From<i32> for DogValue {
    fn from(n: i32) -> Self { DogValue::Integer(n as i64) }
}
#[cfg(all(feature = "serde", not(feature = "json")))]
impl From<u32> for DogValue {
    fn from(n: u32) -> Self { DogValue::Integer(n as i64) }
}
#[cfg(all(feature = "serde", not(feature = "json")))]
impl From<Vec<DogValue>> for DogValue {
    fn from(a: Vec<DogValue>) -> Self { DogValue::Array(a) }
}
#[cfg(all(feature = "serde", not(feature = "json")))]
impl From<std::collections::BTreeMap<String, DogValue>> for DogValue {
    fn from(m: std::collections::BTreeMap<String, DogValue>) -> Self { DogValue::Object(m) }
}

/// A structured DogRS error that can live inside `anyhow::Error`.
///
/// Mirrors Feathers-style fields:
/// - name
/// - message
/// - code (HTTP status)
/// - class_name
/// - data (optional)
/// - errors (optional)
#[derive(Debug)]
pub struct DogError {
    pub kind: ErrorKind,
    pub message: String,
    pub data: Option<ErrorValue>,
    pub errors: Option<ErrorValue>,
    /// Internal error chain — private to prevent accidental exposure over the wire.
    ///
    /// Use [`DogError::source_ref`] to read, [`DogError::into_source`] to consume,
    /// or [`DogError::sanitize_for_client`] before serialising to a client.
    source: Option<AnyError>,
}

impl DogError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            data: None,
            errors: None,
            source: None,
        }
    }

    /// Attach arbitrary payload to the error (e.g. the failing resource).
    #[must_use = "builder returns a new DogError — assign the result or the data is lost"]
    pub fn with_data(mut self, data: ErrorValue) -> Self {
        self.data = Some(data);
        self
    }

    /// Attach field-level validation errors (e.g. `{"email": ["required"]}`)
    #[must_use = "builder returns a new DogError — assign the result or the errors are lost"]
    pub fn with_errors(mut self, errors: ErrorValue) -> Self {
        self.errors = Some(errors);
        self
    }

    /// Attach the originating error for internal logging (never serialised to clients).
    #[must_use = "builder returns a new DogError — assign the result or the source is lost"]
    pub fn with_source(mut self, source: anyhow::Error) -> Self {
        self.source = Some(source);
        self
    }

    /// Borrow the internal error source, if one is attached.
    ///
    /// For **logging only** — never include source details in a wire response.
    /// Call [`sanitize_for_client`](Self::sanitize_for_client) before serialising.
    pub fn source_ref(&self) -> Option<&AnyError> {
        self.source.as_ref()
    }

    /// Consume this `DogError` and take its source, if one is attached.
    ///
    /// Useful when you need to re-raise the original cause after inspection.
    pub fn into_source(self) -> Option<AnyError> {
        self.source
    }

    /// Returns `true` if an internal source error is attached.
    pub fn has_source(&self) -> bool {
        self.source.is_some()
    }

    pub fn code(&self) -> u16 {
        self.kind.status_code()
    }

    pub fn name(&self) -> &'static str {
        self.kind.name()
    }

    pub fn class_name(&self) -> &'static str {
        self.kind.class_name()
    }

    /// Convert into `anyhow::Error` so it flows through your hook pipeline.
    #[must_use = "this returns the wrapped error — return or propagate it"]
    pub fn into_anyhow(self) -> AnyError {
        AnyError::new(self)
    }

    /// Find a `DogError` inside an `anyhow::Error` chain, if one exists.
    ///
    /// Walks the full source chain (including `.context()` wrappers) using
    /// `chain().find_map()` — consistent with how `normalize()` and `dog-axum`
    /// locate `DogError` in a mixed error chain.
    ///
    /// Note: `downcast_ref()` alone only checks the **root** type — a `DogError`
    /// buried under `.context()` would return `None`. Use this method instead.
    #[must_use = "inspect or use the returned reference; ignoring it is a no-op"]
    pub fn from_anyhow(err: &AnyError) -> Option<&DogError> {
        err.chain().find_map(|e| e.downcast_ref::<DogError>())
    }

    /// Turn any error into a DogError:
    /// - if it's already a DogError at the root, own it directly (lossless, zero overhead)
    /// - if a DogError is buried under `.context()` wrappers, extract its `kind`, `message`,
    ///   `data`, and `errors` — then keep the full wrapped error as `source`
    /// - otherwise wrap as `GeneralError`
    ///
    /// Uses `chain().find_map()` (NOT plain `downcast_ref`, which only checks the root type)
    /// so `.context("...")` wrappers do not silently demote a `DogError` to `GeneralError`.
    #[must_use = "normalize returns a new DogError — assign and use it"]
    pub fn normalize(err: AnyError) -> DogError {
        // Fast path: root is already DogError — own it directly (lossless)
        match err.downcast::<DogError>() {
            Ok(dog) => dog,
            Err(other) => {
                // Slow path: walk the source chain for a buried DogError
                // (.context() wraps the original error as a source, not the root)
                if let Some(dog_ref) = other.chain().find_map(|e| e.downcast_ref::<DogError>()) {
                    // Clone all user-visible fields BEFORE moving `other` into with_source().
                    // NLL ends the borrow of `other` via `dog_ref` after the last clone here.
                    let kind = dog_ref.kind;
                    let message = dog_ref.message.clone();
                    let data = dog_ref.data.clone();
                    let errors = dog_ref.errors.clone();
                    // `dog_ref` borrow ends here — safe to move `other` below
                    let mut reconstructed = DogError::new(kind, message);
                    if let Some(d) = data    { reconstructed = reconstructed.with_data(d); }
                    if let Some(e) = errors  { reconstructed = reconstructed.with_errors(e); }
                    reconstructed.with_source(other)
                } else {
                    DogError::general_error(other.to_string()).with_source(other)
                }
            }
        }
    }

    /// A “safe” version suitable for returning to clients:
    /// - keep kind/message/code/class_name/data/errors
    /// - drop the inner `source` (stack/secret details)
    #[must_use = "sanitize_for_client returns a new DogError with source stripped — use that, not the original"]
    pub fn sanitize_for_client(&self) -> DogError {
        DogError {
            kind: self.kind,
            message: self.message.clone(),
            data: self.data.clone(),
            errors: self.errors.clone(),
            source: None,
        }
    }

    // ---- Constructors (Feathers-style) ----

    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::BadRequest, msg)
    }
    pub fn not_authenticated(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotAuthenticated, msg)
    }
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Forbidden, msg)
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, msg)
    }
    pub fn method_not_allowed(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::MethodNotAllowed, msg)
    }
    pub fn not_acceptable(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotAcceptable, msg)
    }
    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Timeout, msg)
    }
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Conflict, msg)
    }
    pub fn gone(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Gone, msg)
    }
    pub fn length_required(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::LengthRequired, msg)
    }
    pub fn unprocessable(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unprocessable, msg)
    }
    pub fn too_many_requests(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::TooManyRequests, msg)
    }
    pub fn general_error(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::GeneralError, msg)
    }
    pub fn not_implemented(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotImplemented, msg)
    }
    pub fn bad_gateway(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::BadGateway, msg)
    }
    pub fn unavailable(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unavailable, msg)
    }
}

impl fmt::Display for DogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}): {}", self.name(), self.code(), self.message)
    }
}

impl std::error::Error for DogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for DogError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut fields = 4;
        if self.data.is_some() { fields += 1; }
        if self.errors.is_some() { fields += 1; }

        let mut state = serializer.serialize_struct("DogError", fields)?;
        state.serialize_field("name", self.name())?;
        state.serialize_field("message", &self.message)?;
        state.serialize_field("code", &self.code())?;
        state.serialize_field("className", self.class_name())?;
        if let Some(d) = &self.data {
            state.serialize_field("data", d)?;
        }
        if let Some(e) = &self.errors {
            state.serialize_field("errors", e)?;
        }
        state.end()
    }
}

#[cfg(feature = "json")]
impl DogError {
    /// Feathers-ish JSON payload — delegates to the `Serialize` impl so there
    /// is a single source of truth for the field layout.
    pub fn to_json(&self) -> serde_json::Value {
        // `json` implies `serde` (via feature dep), so Serialize is always available
        // here. The only error path in DogError::serialize is emitting &str / u16 /
        // serde_json::Value — none of which can fail.
        serde_json::to_value(self).expect("DogError serialization is infallible")
    }
}


/// Convenience helper for "bail with DogError".
///
/// # Examples
/// ```ignore
/// bail_dog!(not_found, "user {} not found", id);
/// bail_dog!(unprocessable, "validation failed", errors = my_value);
/// bail_dog!(bad_request, "hint included", data = hint_value);
/// bail_dog!(unprocessable, "failed", data = data_val, errors = errors_val);
/// bail_dog!(unprocessable, "failed", errors = errors_val, data = data_val); // either ordering
/// ```
///
/// # Known Limitations
/// The tokens `errors` and `data` are reserved in the trailing position — they
/// trigger the metadata arms rather than the format-string arm.
/// If you need to use them as named format arguments, wrap the message manually:
/// ```ignore
/// // This conflicts with the `errors = ...` arm:
/// // bail_dog!(not_found, "{errors}", errors = count);  // WON'T COMPILE
/// // Do this instead:
/// bail_dog!(not_found, "{}", format_args!("{errors}", errors = count));
/// // Or:
/// let msg = format!("{errors}", errors = count);
/// bail_dog!(not_found, msg);
/// ```
#[macro_export]
macro_rules! bail_dog {
    // Simple message
    ($ctor:ident, $msg:expr) => {
        return Err($crate::errors::DogError::$ctor($msg).into_anyhow());
    };
    // With errors metadata only
    ($ctor:ident, $msg:expr, errors = $errors:expr) => {
        return Err($crate::errors::DogError::$ctor($msg).with_errors($errors).into_anyhow());
    };
    // With data metadata only
    ($ctor:ident, $msg:expr, data = $data:expr) => {
        return Err($crate::errors::DogError::$ctor($msg).with_data($data).into_anyhow());
    };
    // With both: data first, then errors
    ($ctor:ident, $msg:expr, data = $data:expr, errors = $errors:expr) => {
        return Err($crate::errors::DogError::$ctor($msg).with_data($data).with_errors($errors).into_anyhow());
    };
    // With both: errors first, then data — mirrors the data+errors arm for either ordering
    ($ctor:ident, $msg:expr, errors = $errors:expr, data = $data:expr) => {
        return Err($crate::errors::DogError::$ctor($msg).with_data($data).with_errors($errors).into_anyhow());
    };
    // Format string (must be last — catches remaining $($arg:tt)+ patterns)
    ($ctor:ident, $fmt:expr, $($arg:tt)+) => {
        return Err($crate::errors::DogError::$ctor(format!($fmt, $($arg)+)).into_anyhow());
    };
}

#[cfg(all(test, feature = "json"))]
mod tests {
    use super::*;

    /// `source` carries internal stack traces — it must NEVER appear in the
    /// serialized output sent to clients.
    #[test]
    fn source_does_not_leak_in_serialization() {
        let err = DogError::not_found("user 42 not found")
            .with_source(anyhow::anyhow!("secret internal detail: DB timeout"));
        let json = err.to_json();

        assert!(json.get("source").is_none(), "source must not appear in wire output");
        assert_eq!(json["code"], 404);
        assert_eq!(json["name"], "NotFound");
    }

    /// FeathersJS clients expect `className` (camelCase), not `class_name`.
    /// A regression here silently breaks all frontend error handling.
    #[test]
    fn class_name_is_camel_case_in_wire_output() {
        let json = DogError::not_authenticated("token expired").to_json();

        assert!(json.get("class_name").is_none(), "snake_case must not appear in wire output");
        assert_eq!(json["className"], "not-authenticated");
        assert_eq!(json["code"], 401);
    }

    /// `data` and `errors` are omitted when not set (sparse output).
    #[test]
    fn optional_fields_omitted_when_none() {
        let json = DogError::bad_request("missing field").to_json();
        assert!(json.get("data").is_none());
        assert!(json.get("errors").is_none());
    }

    /// `data` and `errors` appear when set.
    #[test]
    fn optional_fields_present_when_set() {
        use serde_json::json;
        let json = DogError::unprocessable("validation failed")
            .with_errors(json!({"email": ["is required"]}))
            .to_json();

        assert!(json.get("errors").is_some());
        assert_eq!(json["errors"]["email"][0], "is required");
    }
}

/// Tests for the serde-only path (`default-features = false, features = ["serde"]`).
/// Run with: cargo test -p dog-core --no-default-features --features serde
#[cfg(all(test, feature = "serde", not(feature = "json")))]
mod tests_serde_only {
    use super::*;
    use serde_json; // only for comparison — not the ErrorValue type here

    #[test]
    fn dog_value_integer_preserves_large_values() {
        // f64 can only represent integers exactly up to 2^53.
        // DogValue::Integer(i64) must handle values beyond that without corruption.
        let large_id: i64 = 9_007_199_254_740_993; // 2^53 + 1
        let v = DogValue::Integer(large_id);
        // Serialize to JSON (self-describing) to verify the value round-trips
        let json_str = serde_json::to_string(&v).unwrap();
        assert_eq!(json_str, large_id.to_string());
    }

    #[test]
    fn dog_value_integer_and_float_are_distinct() {
        // Separate variants prevent accidentally using 42.0f64 for an integer ID
        let i = DogValue::Integer(42);
        let f = DogValue::Float(42.0);
        let si = serde_json::to_string(&i).unwrap();
        let sf = serde_json::to_string(&f).unwrap();
        // Integer serializes as 42, Float as 42.0
        assert_eq!(si, "42");
        assert!(sf.contains('.'), "Float must include decimal point: {}", sf);
    }

    #[test]
    fn dog_error_serialize_with_dog_value_errors() {
        let err = DogError::unprocessable("validation failed")
            .with_errors(DogValue::Object(std::collections::BTreeMap::from([
                ("email".into(), DogValue::Array(vec![
                    DogValue::String("is required".into()),
                ])),
            ])));
        // Serialize to JSON for inspection (serde_json serializer works with Serialize impl)
        let json_str = serde_json::to_string(&err).unwrap();
        assert!(json_str.contains("\"className\":\"unprocessable\""));
        assert!(json_str.contains("is required"));
        assert!(!json_str.contains("source"), "source must not leak");
    }

    #[test]
    fn dog_value_float_validated_constructor() {
        assert!(DogValue::float(1.5).is_some());
        assert!(DogValue::float(f64::NAN).is_none(), "NaN must be rejected");
        assert!(DogValue::float(f64::INFINITY).is_none(), "Infinity must be rejected");
        assert!(DogValue::float(f64::NEG_INFINITY).is_none(), "Neg infinity must be rejected");
    }

    #[test]
    fn dog_value_from_impls() {
        assert_eq!(DogValue::from("hello"), DogValue::String("hello".to_owned()));
        assert_eq!(DogValue::from(42i64), DogValue::Integer(42));
        assert_eq!(DogValue::from(42i32), DogValue::Integer(42));
        assert_eq!(DogValue::from(42u32), DogValue::Integer(42));
        assert_eq!(DogValue::from(true), DogValue::Bool(true));
    }

    /// Round-trip all 7 DogValue variants through JSON.
    /// Serialize uses #[serde(untagged)] → clean JSON.
    /// Deserialize uses the untagged Visitor → reconstructs the tree.
    #[test]
    fn dog_value_json_roundtrip_all_variants() {
        let cases: &[(&str, DogValue)] = &[
            ("null",   DogValue::Null),
            ("true",   DogValue::Bool(true)),
            ("false",  DogValue::Bool(false)),
            ("42",     DogValue::Integer(42)),
            ("42.5",   DogValue::Float(42.5)),
            ("\"hi\"", DogValue::String("hi".to_owned())),
        ];
        for (expected_json, value) in cases {
            let serialized = serde_json::to_string(value).unwrap();
            assert_eq!(&serialized, expected_json, "Serialize mismatch for {:?}", value);
            let deserialized: DogValue = serde_json::from_str(&serialized).unwrap();
            assert_eq!(&deserialized, value, "Round-trip mismatch for {:?}", value);
        }
    }

    /// Integer 42 (no decimal) must deserialize as Integer, not Float.
    /// Float 42.0 (with decimal) must deserialize as Float, not Integer.
    /// This verifies the variant ordering: Integer is tried before Float.
    #[test]
    fn dog_value_integer_float_deserialization_disambiguation() {
        let i: DogValue = serde_json::from_str("42").unwrap();
        assert_eq!(i, DogValue::Integer(42), "42 must deserialize as Integer");

        let f: DogValue = serde_json::from_str("42.0").unwrap();
        assert_eq!(f, DogValue::Float(42.0), "42.0 must deserialize as Float");
    }

    /// Large i64 above f64 precision boundary must survive round-trip losslessly.
    #[test]
    fn dog_value_large_integer_roundtrip() {
        let large: i64 = 9_007_199_254_740_993; // 2^53 + 1 — corrupted by f64
        let v = DogValue::Integer(large);
        let s = serde_json::to_string(&v).unwrap();
        let back: DogValue = serde_json::from_str(&s).unwrap();
        assert_eq!(back, v, "Large i64 must round-trip losslessly through JSON");
    }

    /// Complex nested payload (Object > Array > primitives) round-trip.
    #[test]
    fn dog_value_nested_payload_roundtrip() {
        use std::collections::BTreeMap;
        let payload = DogValue::Object(BTreeMap::from([
            ("email".into(), DogValue::Array(vec![
                DogValue::String("is required".into()),
                DogValue::String("must be valid".into()),
            ])),
            ("count".into(), DogValue::Integer(3)),
            ("flagged".into(), DogValue::Bool(true)),
        ]));
        let json = serde_json::to_string(&payload).unwrap();
        let back: DogValue = serde_json::from_str(&json).unwrap();
        assert_eq!(back, payload);
    }
}

/// Tests for normalize() chain-walking — run under default features (json)
/// Tests for normalize() — no serde/json dependency; run under any feature configuration
#[cfg(test)]
mod tests_normalize {
    use super::*;

    #[test]
    fn normalize_root_dog_error_is_preserved_losslessly() {
        let original = DogError::not_found("user 42");
        let err = original.into_anyhow();
        let n = DogError::normalize(err);
        assert_eq!(n.kind, ErrorKind::NotFound);
        assert_eq!(n.message, "user 42");
    }

    #[test]
    fn normalize_context_wrapped_dog_error_preserves_kind_and_message() {
        // This is the bug that was fixed — .context() wraps as ContextError root,
        // so plain downcast::<DogError>() fails. chain().find_map() finds it.
        let err = DogError::not_found("user 42")
            .into_anyhow()
            .context("while fetching user profile");
        let n = DogError::normalize(err);
        // Kind and message from the original DogError must be preserved
        assert_eq!(n.kind, ErrorKind::NotFound, "kind must survive .context() wrapping");
        assert_eq!(n.message, "user 42", "message must survive .context() wrapping");
    }

    #[test]
    fn normalize_non_dog_error_becomes_general_error() {
        let err: AnyError = anyhow::anyhow!("plain anyhow error");
        let n = DogError::normalize(err);
        assert_eq!(n.kind, ErrorKind::GeneralError);
        assert!(n.message.contains("plain anyhow error"));
    }

    /// Regression test for silent data/errors drop in the slow path.
    /// Before the fix, a context-wrapped DogError with .with_data()/.with_errors()
    /// would lose those fields in normalize() — only kind+message were preserved.
    #[cfg(feature = "json")]
    #[test]
    fn normalize_context_wrapped_dog_error_preserves_data_and_errors() {
        let err = DogError::unprocessable("validation failed")
            .with_data(serde_json::json!({"hint": "check format"}))
            .with_errors(serde_json::json!({"email": ["required"]}))
            .into_anyhow()
            .context("in request handler");
        let n = DogError::normalize(err);
        assert_eq!(n.kind, ErrorKind::Unprocessable);
        assert_eq!(n.message, "validation failed");
        assert!(n.data.is_some(),   "data must survive .context() wrapping in normalize()");
        assert!(n.errors.is_some(), "errors must survive .context() wrapping in normalize()");
    }
}

/// Tests for bail_dog! — covers all macro arms.
/// These tests would NOT have required serde; run under any feature configuration.
///
/// Run with: `cargo test -p dog-core bail_dog`
#[cfg(test)]
mod tests_bail_dog {
    use super::*;

    fn try_simple() -> DogResult<()> {
        bail_dog!(not_found, "item missing");
    }

    fn try_data_only() -> DogResult<()> {
        #[cfg(feature = "json")]
        bail_dog!(bad_request, "data only",
            data = serde_json::json!({"hint": "check format"}));
        #[cfg(all(feature = "serde", not(feature = "json")))]
        bail_dog!(bad_request, "data only",
            data = DogValue::String("hint".into()));
        #[cfg(not(any(feature = "serde", feature = "json")))]
        bail_dog!(bad_request, "data only");
        #[allow(unreachable_code)]
        Ok(())
    }

    fn try_errors_only() -> DogResult<()> {
        #[cfg(feature = "json")]
        bail_dog!(unprocessable, "validation failed",
            errors = serde_json::json!({"email": ["required"]}));
        #[cfg(all(feature = "serde", not(feature = "json")))]
        bail_dog!(unprocessable, "validation failed",
            errors = DogValue::String("required".into()));
        #[cfg(not(any(feature = "serde", feature = "json")))]
        bail_dog!(unprocessable, "validation failed");
        // Unreachable — all arms bail
        #[allow(unreachable_code)]
        Ok(())
    }

    fn try_data_then_errors() -> DogResult<()> {
        #[cfg(feature = "json")]
        bail_dog!(bad_request, "bad payload",
            data    = serde_json::json!({"hint": "check format"}),
            errors  = serde_json::json!({"field": ["invalid"]}));
        #[cfg(all(feature = "serde", not(feature = "json")))]
        bail_dog!(bad_request, "bad payload",
            data    = DogValue::String("hint".into()),
            errors  = DogValue::String("invalid".into()));
        #[cfg(not(any(feature = "serde", feature = "json")))]
        bail_dog!(bad_request, "bad payload");
        #[allow(unreachable_code)]
        Ok(())
    }

    /// This arm was MISSING before the fix and would have caused a confusing
    /// `format!` compile error — errors=, data= ordering is now supported.
    fn try_errors_then_data() -> DogResult<()> {
        #[cfg(feature = "json")]
        bail_dog!(unprocessable, "reversed order",
            errors  = serde_json::json!({"field": ["invalid"]}),
            data    = serde_json::json!({"hint": "check format"}));
        #[cfg(all(feature = "serde", not(feature = "json")))]
        bail_dog!(unprocessable, "reversed order",
            errors  = DogValue::String("invalid".into()),
            data    = DogValue::String("hint".into()));
        #[cfg(not(any(feature = "serde", feature = "json")))]
        bail_dog!(unprocessable, "reversed order");
        #[allow(unreachable_code)]
        Ok(())
    }

    fn try_format_string() -> DogResult<()> {
        let id = 42u32;
        bail_dog!(not_found, "user {} not found", id);
    }

    #[test]
    fn bail_dog_simple_message() {
        let err = try_simple().unwrap_err();
        let dog = DogError::from_anyhow(&err).expect("must be DogError");
        assert_eq!(dog.kind, ErrorKind::NotFound);
        assert_eq!(dog.message, "item missing");
    }

    /// Data-only arm (arm 3). Guarded: 'data =' requires an ErrorValue.
    #[cfg(any(feature = "serde", feature = "json"))]
    #[test]
    fn bail_dog_data_only() {
        let err = try_data_only().unwrap_err();
        let dog = DogError::from_anyhow(&err).expect("must be DogError");
        assert_eq!(dog.kind, ErrorKind::BadRequest);
        assert!(dog.data.is_some(), "data must be attached");
        assert!(dog.errors.is_none(), "errors must be absent");
    }

    /// Errors-only arm (arm 2). Guarded: 'errors =' requires an ErrorValue.
    #[cfg(any(feature = "serde", feature = "json"))]
    #[test]
    fn bail_dog_errors_metadata() {
        let err = try_errors_only().unwrap_err();
        let dog = DogError::from_anyhow(&err).expect("must be DogError");
        assert_eq!(dog.kind, ErrorKind::Unprocessable);
        assert_eq!(dog.message, "validation failed");
        assert!(dog.errors.is_some(), "errors must be attached");
        assert!(dog.data.is_none(), "data must be absent");
    }

    /// Data+errors arm (arm 4). Guarded: requires an ErrorValue.
    #[cfg(any(feature = "serde", feature = "json"))]
    #[test]
    fn bail_dog_data_then_errors() {
        let err = try_data_then_errors().unwrap_err();
        let dog = DogError::from_anyhow(&err).expect("must be DogError");
        assert_eq!(dog.kind, ErrorKind::BadRequest);
        assert!(dog.data.is_some(), "data must be attached");
        assert!(dog.errors.is_some(), "errors must be attached");
    }

    /// Verifies the formerly-missing reverse-order arm (arm 5). Guarded: requires an ErrorValue.
    #[cfg(any(feature = "serde", feature = "json"))]
    #[test]
    fn bail_dog_errors_then_data_reverse_order() {
        let err = try_errors_then_data().unwrap_err();
        let dog = DogError::from_anyhow(&err).expect("must be DogError");
        assert_eq!(dog.kind, ErrorKind::Unprocessable);
        assert_eq!(dog.message, "reversed order");
        assert!(dog.data.is_some(), "data must be attached even in reverse order");
        assert!(dog.errors.is_some(), "errors must be attached even in reverse order");
    }

    #[test]
    fn bail_dog_format_string() {
        let err = try_format_string().unwrap_err();
        let dog = DogError::from_anyhow(&err).expect("must be DogError");
        assert_eq!(dog.kind, ErrorKind::NotFound);
        assert_eq!(dog.message, "user 42 not found");
    }
}
