//! # Errors (Feathers-style)
//!
//! DogRS provides a Feathers-inspired set of structured errors.
//! Core goals:
//! - consistent status codes + class names
//! - can be carried through anyhow::Error (for hook pipeline)
//! - transport-agnostic (server crate decides how to serialize)
//!
//! If you enable feature `serde`, you also get:
//! - `data` / `errors` as serde_json::Value
//! - `to_json()` helper

use std::fmt;

use anyhow::Error as AnyError;

/// A convenience result type for DogRS core APIs.
pub type DogResult<T> = std::result::Result<T, AnyError>;

/// Feathers-ish error class names + status codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

#[cfg(feature = "serde")]
pub type ErrorValue = serde_json::Value;

#[cfg(not(feature = "serde"))]
pub type ErrorValue = std::sync::Arc<dyn std::any::Any + Send + Sync>;

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
    pub source: Option<AnyError>,
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

    pub fn with_data(mut self, data: ErrorValue) -> Self {
        self.data = Some(data);
        self
    }

    pub fn with_errors(mut self, errors: ErrorValue) -> Self {
        self.errors = Some(errors);
        self
    }

    pub fn with_source(mut self, source: AnyError) -> Self {
        self.source = Some(source);
        self
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
    pub fn into_anyhow(self) -> AnyError {
        AnyError::new(self)
    }

    /// Downcast an `anyhow::Error` to a `DogError` if possible.
    pub fn from_anyhow(err: &AnyError) -> Option<&DogError> {
        err.downcast_ref::<DogError>()
    }

    /// Turn any error into a DogError:
    /// - if it’s already a DogError, keep it (lossless)
    /// - otherwise wrap as GeneralError
    pub fn normalize(err: AnyError) -> DogError {
        match err.downcast::<DogError>() {
            Ok(dog) => dog,
            Err(other) => DogError::new(ErrorKind::GeneralError, other.to_string()).with_source(other),
        }
    }

    /// A “safe” version suitable for returning to clients:
    /// - keep kind/message/code/class_name/data/errors
    /// - drop the inner `source` (stack/secret details)
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
impl DogError {
    /// Feathers-ish JSON payload.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;

        let mut base = json!({
            "name": self.name(),
            "message": self.message,
            "code": self.code(),
            "className": self.class_name(),
        });

        if let Some(d) = &self.data {
            base["data"] = d.clone();
        }
        if let Some(e) = &self.errors {
            base["errors"] = e.clone();
        }
        base
    }
}

/// Convenience trait: convert a `DogError` into `anyhow::Error`.
pub trait IntoAnyhowDogError {
    fn into_anyhow(self) -> AnyError;
}

impl IntoAnyhowDogError for DogError {
    fn into_anyhow(self) -> AnyError {
        self.into_anyhow()
    }
}

/// Convenience helper for “bail with DogError”.
#[macro_export]
macro_rules! bail_dog {
    ($ctor:ident, $msg:expr) => {
        return Err($crate::errors::DogError::$ctor($msg).into_anyhow());
    };
    ($ctor:ident, $fmt:expr, $($arg:tt)*) => {
        return Err($crate::errors::DogError::$ctor(format!($fmt, $($arg)*)).into_anyhow());
    };
}
