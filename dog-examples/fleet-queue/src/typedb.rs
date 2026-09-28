use super::FleetParams;
use anyhow::Result;
use dog_core::DogAppBuilder;
use dog_typedb::{
    adapter::TypeDBState as TypeDBStateTrait, execute_typedb_query, load_schema_from_file,
};
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
    pub async fn setup_db(app: &mut DogAppBuilder<serde_json::Value, FleetParams>) -> Result<()> {
        let address = std::env::var("TYPEDB_ADDRESS")
            .ok()
            .or_else(|| app.get::<String>("typedb.address"))
            .unwrap_or_else(|| "127.0.0.1:1729".into());
        let database = std::env::var("TYPEDB_DATABASE")
            .ok()
            .or_else(|| app.get::<String>("typedb.database"))
            .unwrap_or_else(|| "fleet-db".into());
        let local = address
            .parse::<std::net::SocketAddr>()
            .is_ok_and(|a| a.ip().is_loopback())
            || address
                .strip_prefix("localhost:")
                .is_some_and(|p| p.parse::<u16>().is_ok());
        let username = std::env::var("TYPEDB_USERNAME")
            .ok()
            .or_else(|| app.get::<String>("typedb.username"))
            .or_else(|| local.then(|| "admin".into()))
            .ok_or_else(|| anyhow::anyhow!("remote TypeDB requires TYPEDB_USERNAME"))?;
        let password = std::env::var("TYPEDB_PASSWORD")
            .ok()
            .or_else(|| app.get::<String>("typedb.password"))
            .or_else(|| local.then(|| "password".into()))
            .ok_or_else(|| anyhow::anyhow!("remote TypeDB requires TYPEDB_PASSWORD"))?;
        let tls = match std::env::var("TYPEDB_TLS")
            .ok()
            .or_else(|| app.get::<String>("typedb.tls"))
        {
            Some(value) => value.parse::<bool>()?,
            None => !local,
        };
        anyhow::ensure!(local || tls, "remote TypeDB requires verified TLS");

        let credentials = Credentials::new(&username, &password);
        let tls_config = if tls {
            DriverTlsConfig::default()
        } else {
            DriverTlsConfig::disabled()
        };
        let options = DriverOptions::new(tls_config);
        let addresses = Addresses::try_from_address_str(&address)?;
        let driver = Arc::new(TypeDBDriver::new(addresses, credentials, options).await?);

        let exists = driver
            .databases()
            .all()
            .await?
            .iter()
            .any(|db| db.name() == database);
        let initialize = std::env::var("TYPEDB_INIT_SCHEMA").as_deref() == Ok("1");
        if !exists {
            driver.databases().create(&database).await?;
        }
        let state = Arc::new(Self { driver, database });
        // Existing databases are migrated explicitly, not on every server restart.
        if !exists || initialize {
            Self::load_schema_from_file(&state).await?;
        }
        app.set("typedb", state);
        Ok(())
    }

    async fn load_schema_from_file(state: &TypeDBState) -> Result<()> {
        let schema_paths = [concat!(env!("CARGO_MANIFEST_DIR"), "/src/")];

        load_schema_from_file(&state.driver, &state.database, &schema_paths).await?;

        // Redefine parameterised functions (TypeDB 3.0 requires explicit parameter signatures)
        let redefine_queries = [
            "redefine fun hours_exceeded_employees($maxHours: double) -> { employee }: match $employee isa employee, has daily-hours $hours; $hours >= $maxHours; return { $employee };",
            "redefine fun compliant_employees($maxHours: double) -> { employee }: match $employee isa employee, has daily-hours $hours; $hours < $maxHours; return { $employee };",
        ];

        for redefine_query in &redefine_queries {
            execute_typedb_query(&state.driver, &state.database, redefine_query).await?;
        }

        Ok(())
    }
}
