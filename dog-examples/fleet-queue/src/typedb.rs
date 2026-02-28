use std::sync::Arc;
use anyhow::Result;
use dog_core::DogApp;
use typedb_driver::{TypeDBDriver, Credentials, DriverOptions};
use dog_typedb::{adapter::TypeDBState as TypeDBStateTrait, load_schema_from_file, execute_typedb_query};
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
        // Enhanced dog-typedb now automatically loads both schema.tql and functions.tql
        let schema_paths = [
            "src/",
            "dog-examples/fleet-queue/src/",
            "./dog-examples/fleet-queue/src/",
        ];
        
        println!("Loading TypeDB schema and functions using enhanced dog-typedb...");
        
        // Single call now loads both schema.tql and functions.tql automatically
        match load_schema_from_file(&state.driver, &state.database, &schema_paths).await {
            Ok(response) => {
                println!("Enhanced schema loading completed!");
                if let Some(loaded_files) = response.get("ok").and_then(|ok| ok.get("loadedFiles")) {
                    println!("Loaded files: {}", loaded_files);
                }
            }
            Err(e) if e.to_string().contains("already exists") => {
                println!("Schema files already exist, proceeding to redefine specific functions...");
            }
            Err(e) => {
                println!("Failed to load schema files: {}", e);
                return Err(e.into());
            }
        }
        
        // Redefine specific functions to add parameters (TypeDB 3.0 best practice)
        let redefine_queries = [
            "redefine fun hours_exceeded_employees($maxHours: double) -> { employee }: match $employee isa employee, has daily-hours $hours; $hours >= $maxHours; return { $employee };",
            "redefine fun compliant_employees($maxHours: double) -> { employee }: match $employee isa employee, has daily-hours $hours; $hours < $maxHours; return { $employee };",
        ];
        
        let mut redefined_count = 0;
        for redefine_query in &redefine_queries {
            match execute_typedb_query(&state.driver, &state.database, redefine_query).await {
                Ok(_) => {
                    println!("TypeDB function redefined with parameters successfully");
                    redefined_count += 1;
                },
                Err(e) => println!("Function redefine failed: {}", e),
            }
        }
        
        if redefined_count > 0 {
            println!("Successfully redefined {} TypeDB functions with parameters", redefined_count);
        }
        
        Ok(())
    }

}
