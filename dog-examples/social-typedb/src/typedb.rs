use std::sync::Arc;
use tokio::sync::Mutex;

use anyhow::Result;
use dog_core::DogApp;
use typedb_driver::{TypeDBDriver, Credentials, DriverOptions};
use dog_typedb::adapter::TypeDBState as TypeDBStateTrait;

use super::SocialParams;


#[derive(Clone)]
pub struct TypeDBState {
    pub driver: Arc<TypeDBDriver>,
    pub database: String,
    pub operation_mutex: Arc<Mutex<()>>,
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
        let address = app
            .get::<String>("typedb.address")
            .unwrap_or_else(|| "127.0.0.1:1729".to_string());
        let username = app
            .get::<String>("typedb.username")
            .unwrap_or_else(|| "".to_string());
        let password = app
            .get::<String>("typedb.password")
            .unwrap_or_else(|| "".to_string());
        let tls = app
            .get::<String>("typedb.tls")
            .and_then(|s| s.parse::<bool>().ok())
            .unwrap_or(false);
        let database = app
            .get::<String>("typedb.database")
            .unwrap_or_else(|| "social-network".to_string());

        // Connect to TypeDB using the standard pattern from examples
        // Use default credentials for TypeDB CE
        let username = if username.is_empty() { "admin" } else { &username };
        let password = if password.is_empty() { "password" } else { &password };
        
        let credentials = Credentials::new(username, password);
        let options = DriverOptions::new(tls, None).map_err(|e| anyhow::anyhow!("{}", e))?;
        let driver = Arc::new(TypeDBDriver::new(&address, credentials, options).await.map_err(|e| anyhow::anyhow!("{}", e))?);

        // Create database if it doesn't exist
        let databases = driver.databases().all().await.map_err(|e| anyhow::anyhow!("{}", e))?;
        let database_exists = databases.iter().any(|db| db.name() == database);
        
        if !database_exists {
            println!("Creating TypeDB database: {}", database);
            driver.databases().create(&database).await.map_err(|e| anyhow::anyhow!("{}", e))?;
        } else {
            println!("TypeDB database '{}' already exists", database);
        }

        // Setup schema - always load to ensure schema is present
        let state = Arc::new(Self { 
            driver: driver.clone(), 
            database: database.clone(),
            operation_mutex: Arc::new(Mutex::new(())),
        });
        Self::load_schema_from_file(&state).await?;

        // Store the DB instance in the app under key "typedb" using polymorphic app.set
        app.set("typedb", state);

        Ok(())
    }

    async fn load_schema_from_file(state: &TypeDBState) -> Result<()> {
        use dog_typedb::load_schema_from_file;
        
        // Use the mutex to serialize schema loading as well
        let _lock = state.operation_mutex.lock().await;
        
        let possible_paths = [
            "src/schema.tql",
            "dog-examples/social-typedb/src/schema.tql",
            "./dog-examples/social-typedb/src/schema.tql",
        ];
        
        load_schema_from_file(&state.driver, &state.database, &possible_paths).await?;
        Ok(())
    }

}
