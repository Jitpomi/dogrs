use dog_schema::schema;

#[schema(
    service = "messages",
    error_message = "Messages schema validation failed"
)]
pub mod def {

    #[create]
    pub struct CreateMessage {
        #[dog(trim, min_len(1))]
        pub text: String,

        // Relationship existence and authorization belong in the service.
        pub sender: String, // user ID

        #[dog(optional)]
        pub receivers: Option<Vec<String>>, // user IDs
    }

    #[patch]
    pub struct PatchMessage {
        #[dog(optional, trim, min_len(1))]
        pub text: Option<String>,

        #[dog(optional)]
        pub sender: Option<String>, // user ID

        #[dog(optional)]
        pub receivers: Option<Vec<String>>, // user IDs
    }
}

pub use def::*;
