//! Run with TYPEDB_ADDRESS set against a disposable test server.
use dog_typedb::{adapter::TypeDBState, load_schema_from_file, TypeDBAdapter, TypeDBDriverFactory};
use serde_json::json;
use std::sync::Arc;
use typedb_driver::TypeDBDriver;

struct State {
    driver: Arc<TypeDBDriver>,
    database: String,
}
impl TypeDBState for State {
    fn driver(&self) -> &Arc<TypeDBDriver> {
        &self.driver
    }
    fn database(&self) -> &str {
        &self.database
    }
}

#[tokio::test]
#[ignore = "requires disposable TypeDB server; run with --ignored"]
async fn transaction_boundaries_and_atomic_schema_loading() -> anyhow::Result<()> {
    let address = std::env::var("TYPEDB_ADDRESS")?;
    let driver = Arc::new(TypeDBDriverFactory::connect_default(&address).await?);
    let database = format!("dogrs-test-{}", uuid::Uuid::new_v4().simple());
    driver.databases().create(&database).await?;
    let adapter = TypeDBAdapter::new(Arc::new(State {
        driver: driver.clone(),
        database: database.clone(),
    }));
    let result = async {
        adapter
            .schema(json!({"query": "define entity audit-item;"}))
            .await?;
        adapter
            .write(json!({"query": "insert $x isa audit-item;"}))
            .await?;
        assert!(adapter
            .read(json!({"query": "match $x isa audit-item; delete $x;"}))
            .await
            .is_err());
        assert!(adapter
            .write(json!({"query": "define entity forbidden-schema;"}))
            .await
            .is_err());
        let found = adapter
            .read(json!({"query": "match $x isa audit-item; fetch { 'id': iid($x) };"}))
            .await?;
        assert_eq!(found["ok"]["answers"].as_array().unwrap().len(), 1);

        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join("schema.tql"),
            "define entity must-rollback;",
        )?;
        std::fs::write(dir.path().join("functions.tql"), "this is invalid TypeQL")?;
        assert!(
            load_schema_from_file(&driver, &database, &[dir.path().to_str().unwrap()])
                .await
                .is_err()
        );
        // If the first file committed despite the second failing, this insert would succeed.
        assert!(adapter
            .write(json!({"query": "insert $x isa must-rollback;"}))
            .await
            .is_err());
        std::fs::write(
            dir.path().join("functions.tql"),
            "define entity second-type;",
        )?;
        load_schema_from_file(&driver, &database, &[dir.path().to_str().unwrap()]).await?;
        adapter
            .write(json!({"query": "insert $x isa must-rollback; $y isa second-type;"}))
            .await?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    driver.databases().get(&database).await?.delete().await?;
    result
}
