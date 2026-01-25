use dog_schema::schema;

#[schema(service = "messages", error_message = "Messages schema validation failed")]
pub mod messages_schema {

    #[create]
    pub struct CreateMessage {
        #[dog(trim, min_len(1))]
        pub text: String,

        #[dog(relation = "users")]
        pub sender: String, // user ID

        #[dog(optional, relation = "users")]
        pub receivers: Option<Vec<String>>, // user IDs
    }

    #[patch]
    pub struct PatchMessage {
        #[dog(optional, trim, min_len(1))]
        pub text: Option<String>,

        #[dog(optional, relation = "users")]
        pub sender: Option<String>, // user ID

        #[dog(optional, relation = "users")]
        pub receivers: Option<Vec<String>>, // user IDs
    }
}

pub use messages_schema::register;
