use dog_schema::schema;

#[schema(service = "authors", error_message = "Authors schema validation failed", backend = "validator")]
pub mod authors_schema {
    use serde::Deserialize;
    use validator::Validate;

    #[derive(Debug, Deserialize, Validate)]
    #[serde(deny_unknown_fields)]
    pub struct AuthorProfile {
        #[validate(length(min = 2, message = "display_name must be at least 2 chars"))]
        pub display_name: String,
    }

    #[derive(Debug, Deserialize, Validate)]
    #[serde(deny_unknown_fields)]
    pub struct AuthorTag {
        #[validate(email(message = "tag email must be a valid email"))]
        pub email: String,
    }

    #[create]
    #[derive(Debug, Deserialize, Validate)]
    #[serde(deny_unknown_fields)]
    pub struct CreateAuthor {
        #[validate(required(message = "name is required"), length(min = 1, message = "name must not be empty"))]
        pub name: Option<String>,

        #[validate(required(message = "email is required"), email(message = "email must be a valid email"))]
        pub email: Option<String>,

        #[validate(nested)]
        pub profile: AuthorProfile,

        #[validate(nested)]
        pub tags: Vec<AuthorTag>,
    }

    #[patch]
    #[derive(Debug, Deserialize, Validate)]
    #[serde(deny_unknown_fields)]
    pub struct PatchAuthor {
        #[validate(length(min = 1, message = "name must not be empty"))]
        pub name: Option<String>,

        #[validate(email(message = "email must be a valid email"))]
        pub email: Option<String>,
    }
}

pub use authors_schema::register;
