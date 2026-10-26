//! # Event Service Parameters (`services/events/event_params.rs`)
//!
//! Service-specific parameters derived from [`CalendarParams`].

use crate::services::CalendarParams;

/// Parameters scoped specifically to event operations.
#[derive(Clone, Default)]
pub struct EventParams {}

impl From<&CalendarParams> for EventParams {
    fn from(_params: &CalendarParams) -> Self {
        Self {}
    }
}
