//! # Service Types (`services/types.rs`)
//!
//! Defines shared types used across application services:
//! - [`CalendarParams`]: Request-level parameters passed through DogRS transports.
//! - [`CalendarState`]: In-memory application state, multi-tenant event storage,
//!   and reminder queue handles.

use dog_queue::JobId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Application request parameters extracted by HTTP and WebSocket adapters.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CalendarParams {
    /// Transport provider identifier (e.g. "http", "ws").
    pub provider: String,
    /// Request headers map.
    pub headers: HashMap<String, String>,
    /// Query string parameters map.
    pub query: HashMap<String, String>,
    /// Request method (e.g. "GET", "POST").
    pub method: String,
    /// Request URI path.
    pub path: String,
    /// Raw unparsed query string, if present.
    pub raw_query: Option<String>,
}

/// Shared application state managed by the application adapter.
pub struct CalendarState {
    /// Partitioned event store: `tenant_id -> (event_id -> Event)`.
    pub events_by_tenant: RwLock<HashMap<String, HashMap<String, serde_json::Value>>>,
    /// Active reminder jobs mapped by event ID for cancel-wins: `event_id -> JobId`.
    pub reminder_jobs: RwLock<HashMap<String, JobId>>,
    /// Shared background queue handle.
    pub queue: Arc<crate::background::CalendarQueue>,
}
