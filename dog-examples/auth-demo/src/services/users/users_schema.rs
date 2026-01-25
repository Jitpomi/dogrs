use dog_schema::schema;

#[schema(service = "users", error_message = "Users schema validation failed")]
pub mod users_schema {
    #[create]
    pub struct CreateUser {
        #[dog(trim, min_len(1))]
        pub username: String,

        #[dog(trim, min_len(6))]
        pub password: String,
    }

    #[patch]
    pub struct PatchUser {
        #[dog(optional, trim, min_len(1))]
        pub username: Option<String>,

        #[dog(optional, trim, min_len(6))]
        pub password: Option<String>,
    }
}

pub use users_schema::register;
