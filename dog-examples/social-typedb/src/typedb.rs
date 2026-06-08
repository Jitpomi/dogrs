use super::SocialParams;
use anyhow::Result;
use dog_typedb::{adapter::TypeDBState as TypeDBStateTrait, load_schema_from_file};
use std::sync::Arc;
use typedb_driver::{Addresses, Credentials, DriverOptions, DriverTlsConfig, TypeDBDriver};

#[derive(Clone)]
pub struct TypeDBState {
    pub driver: Arc<TypeDBDriver>,
    pub database: String,
}

impl TypeDBStateTrait for TypeDBState {
    fn driver(&self) -> &Arc<TypeDBDriver> {
        &self.driver
    }

    fn database(&self) -> &str {
        &self.database
    }
}

impl TypeDBState {
    pub async fn setup_db(
        app: &mut dog_core::DogAppBuilder<serde_json::Value, SocialParams>,
    ) -> Result<()> {
        let address = app
            .get::<String>("typedb.address")
            .unwrap_or_else(|| "127.0.0.1:1729".to_string());
        let database = app
            .get::<String>("typedb.database")
            .unwrap_or_else(|| "social-network".to_string());
        let username = app
            .get::<String>("typedb.username")
            .unwrap_or_else(|| "admin".to_string());
        let password = app
            .get::<String>("typedb.password")
            .unwrap_or_else(|| "password".to_string());
        let tls = app
            .get::<String>("typedb.tls")
            .and_then(|s| s.parse().ok())
            .unwrap_or(false);

        let credentials = Credentials::new(&username, &password);
        let tls_config = if tls {
            DriverTlsConfig::default()
        } else {
            DriverTlsConfig::disabled()
        };
        let options = DriverOptions::new(tls_config);
        let addresses = Addresses::try_from_address_str(&address)?;
        let driver = Arc::new(TypeDBDriver::new(addresses, credentials, options).await?);

        // Create database if it doesn't exist
        if !driver
            .databases()
            .all()
            .await?
            .iter()
            .any(|db| db.name() == database)
        {
            println!("Creating TypeDB database: {}", database);
            driver.databases().create(&database).await?;
        } else {
            println!("TypeDB database '{}' already exists", database);
        }

        let state = Arc::new(Self { driver, database });

        Self::load_schema_from_file(&state).await?;
        app.set("typedb", state);
        Ok(())
    }

    async fn load_schema_from_file(state: &TypeDBState) -> Result<()> {
        let paths = [
            "src/schema.tql",
            "dog-examples/social-typedb/src/schema.tql",
            "./dog-examples/social-typedb/src/schema.tql",
        ];

        load_schema_from_file(&state.driver, &state.database, &paths).await?;
        Ok(())
    }
}
