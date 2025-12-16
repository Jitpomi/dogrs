use dog_schema::schema;

#[schema(service = "posts", error_message = "Posts schema validation failed")]
pub mod posts_schema {
    #[create]
    pub struct CreatePost {
        #[dog(trim, min_len(1))]
        pub title: String,

        #[dog(trim, min_len(1))]
        pub body: String,

        #[dog(default = false)]
        pub published: bool,
    }

    #[patch]
    pub struct PatchPost {
        #[dog(optional, trim, min_len(1))]
        pub title: Option<String>,

        #[dog(optional, trim, min_len(1))]
        pub body: Option<String>,

        #[dog(optional)]
        pub published: Option<bool>,
    }
}

pub use posts_schema::register;
