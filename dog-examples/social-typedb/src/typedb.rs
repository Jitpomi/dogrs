use std::sync::Arc;
use anyhow::Result;
use dog_core::DogApp;
use typedb_driver::{Addresses, TypeDBDriver, Credentials, DriverOptions, DriverTlsConfig};
use dog_typedb::{adapter::TypeDBState as TypeDBStateTrait, load_schema_from_file};
use super::SocialParams;


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
    pub async fn setup_db(app: &DogApp<serde_json::Value, SocialParams>) -> Result<()> {
        let address = app.get::<String>("typedb.address").unwrap_or_else(|| format!("127.0.0.1:1729"));
        let database = app.get::<String>("typedb.database").unwrap_or_else(|| format!("social-network"));
        let username = app.get::<String>("typedb.username").unwrap_or_else(|| format!("admin"));
        let password = app.get::<String>("typedb.password").unwrap_or_else(|| format!("password"));
        let tls = app.get::<String>("typedb.tls").and_then(|s| s.parse().ok()).unwrap_or(false);

        let credentials = Credentials::new(&username, &password);
        let tls_config = if tls { DriverTlsConfig::default() } else { DriverTlsConfig::disabled() };
        let options = DriverOptions::new(tls_config);
        let addresses = Addresses::try_from_address_str(&address).map_err(|e| anyhow::anyhow!(e))?;
        let driver = Arc::new(TypeDBDriver::new(addresses, credentials, options).await?);

        // Create database if it doesn't exist
        if !driver.databases().all().await?.iter().any(|db| db.name() == database) {
            println!("Creating TypeDB database: {}", database);
            driver.databases().create(&database).await?;
        } else {
            println!("TypeDB database '{}' already exists", database);
        }

        let state = Arc::new(Self { 
            driver, 
            database,
        });
        
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
