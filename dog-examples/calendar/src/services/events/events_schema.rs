//! # Events Validation Schema (`services/events/events_schema.rs`)
//!
//! Declarative schema validation rules powered by `dog-schema`:
//! - [`def::CreateEvent`]: Validates event creation input (`title` required with min_len 1; `description`, `start_time`, `end_time`, `location`, `meeting_url`, `color`, `category`, and `recurrence` optional).
//! - [`def::PatchEvent`]: Validates partial event updates (all fields optional).

use dog_schema::schema;

#[schema(service = "events", error_message = "Events schema validation failed")]
pub mod def {
    #[create]
    pub struct CreateEvent {
        #[dog(trim, min_len(1), max_len(200))]
        pub title: String,

        #[dog(optional, trim, min_len(1), max_len(4000))]
        pub description: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(100))]
        pub start_time: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(100))]
        pub end_time: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(200))]
        pub location: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(500))]
        pub meeting_url: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(50))]
        pub color: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(50))]
        pub category: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(200))]
        pub recurrence: Option<String>,
    }

    #[patch]
    pub struct PatchEvent {
        #[dog(optional, trim, min_len(1), max_len(200))]
        pub title: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(4000))]
        pub description: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(100))]
        pub start_time: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(100))]
        pub end_time: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(200))]
        pub location: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(500))]
        pub meeting_url: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(50))]
        pub color: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(50))]
        pub category: Option<String>,

        #[dog(optional, trim, min_len(1), max_len(200))]
        pub recurrence: Option<String>,
    }
}

pub use def::*;
