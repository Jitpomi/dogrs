use std::sync::Arc;
use anyhow::Result;
use dog_core::DogApp;
use typedb_driver::{TypeDBDriver, Credentials, DriverOptions};
use dog_typedb::{adapter::TypeDBState as TypeDBStateTrait, load_schema_from_file};
use super::FleetParams;


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
    pub async fn setup_db(app: &DogApp<serde_json::Value, FleetParams>) -> Result<()> {
        let address = app.get::<String>("typedb.address").unwrap_or_else(|| format!("127.0.0.1:1729"));
        let database = app.get::<String>("typedb.database").unwrap_or_else(|| format!("fleet-db"));
        let username = app.get::<String>("typedb.username").unwrap_or_else(|| format!("admin"));
        let password = app.get::<String>("typedb.password").unwrap_or_else(|| format!("password"));
        let tls = app.get::<String>("typedb.tls").and_then(|s| s.parse().ok()).unwrap_or(false);

        let credentials = Credentials::new(&username, &password);
        let options = DriverOptions::new(tls, None)?;
        let driver = Arc::new(TypeDBDriver::new(&address, credentials, options).await?);

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
            "dog-examples/fleet-queue/src/schema.tql",
            "./dog-examples/fleet-queue/src/schema.tql",
        ];
        
        load_schema_from_file(&state.driver, &state.database, &paths).await?;
        Ok(())
    }

}
