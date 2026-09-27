//! Run with TYPEDB_ADDRESS set against a disposable test server.
use dog_typedb::{adapter::TypeDBState, load_schema_from_file, TypeDBAdapter, TypeDBDriverFactory};
use futures::FutureExt;
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
    let result = std::panic::AssertUnwindSafe(async {
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

        // Returning too many rows must not commit a partially consumed write.
        adapter
            .schema(json!({"query":"define entity capacity-item;"}))
            .await?;
        for _ in 0..2 {
            adapter
                .write(json!({"query":"insert $x isa capacity-item;"}))
                .await?;
        }
        let limited = TypeDBAdapter::new(Arc::new(State {
            driver: driver.clone(),
            database: database.clone(),
        }))
        .with_options(dog_typedb::QueryOptions {
            max_answers: 1,
            ..Default::default()
        })?;
        assert!(limited
            .read(json!({"query":"match $x isa capacity-item;"}))
            .await
            .unwrap_err()
            .to_string()
            .contains("Answer count limit"));
        assert!(limited
            .write(json!({"query":"match $x isa capacity-item; insert $y isa capacity-item;"}))
            .await
            .unwrap_err()
            .to_string()
            .contains("Answer count limit"));
        let found = adapter
            .read(json!({"query":"match $x isa capacity-item;"}))
            .await?;
        assert_eq!(found["ok"]["answers"].as_array().unwrap().len(), 2);
        let tiny = TypeDBAdapter::new(Arc::new(State {
            driver: driver.clone(),
            database: database.clone(),
        }))
        .with_options(dog_typedb::QueryOptions {
            max_response_bytes: 64,
            ..Default::default()
        })?;
        assert!(tiny
            .write(json!({"query":"insert $x isa capacity-item;"}))
            .await
            .is_err());
        assert!(tiny
            .schema(json!({"query":"define entity byte-limit-rollback;"}))
            .await
            .is_err());
        assert!(adapter
            .write(json!({"query":"insert $x isa byte-limit-rollback;"}))
            .await
            .is_err());
        let found = adapter
            .read(json!({"query":"match $x isa capacity-item;"}))
            .await?;
        assert_eq!(found["ok"]["answers"].as_array().unwrap().len(), 2);

        // A single large fetch document must hit the byte budget, not just the row budget.
        adapter.schema(json!({"query":"define attribute payload, value string; entity sized-item, owns payload;"})).await?;
        let large = "x".repeat(4096);
        adapter.write(json!({"query":format!("insert $x isa sized-item, has payload '{large}';")})).await?;
        let bytes_limited = TypeDBAdapter::new(Arc::new(State { driver:driver.clone(), database:database.clone() }))
            .with_options(dog_typedb::QueryOptions { max_response_bytes:1024, ..Default::default() })?;
        let error = bytes_limited.read(json!({"query":"match $x isa sized-item, has payload $p; fetch { 'payload': $p };"})).await.unwrap_err();
        assert!(error.to_string().contains("Response byte limit"));

        // Explicit missing migrations must fail before applying any other file.
        let explicit = dir.path().join("migration-one.tql");
        let missing = dir.path().join("missing.tql");
        std::fs::write(&explicit, "define entity explicit-migration;")?;
        assert!(load_schema_from_file(
            &driver,
            &database,
            &[explicit.to_str().unwrap(), missing.to_str().unwrap()]
        )
        .await
        .is_err());
        assert!(adapter
            .write(json!({"query":"insert $x isa explicit-migration;"}))
            .await
            .is_err());
        let loaded =
            load_schema_from_file(&driver, &database, &[explicit.to_str().unwrap()]).await?;
        assert_eq!(loaded["ok"]["loadedFiles"][0], explicit.to_str().unwrap());
        adapter
            .write(json!({"query":"insert $x isa explicit-migration;"}))
            .await?;

        // A batch response overflow also rolls back definitions from earlier files.
        let batch_a = dir.path().join("batch-a.tql");
        let batch_b = dir.path().join("batch-b.tql");
        std::fs::write(&batch_a, format!("define entity batch-a; #{}", "x".repeat(300)))?;
        std::fs::write(&batch_b, format!("define entity batch-b; #{}", "x".repeat(300)))?;
        let error = dog_typedb::load_schema_with_options(&driver, &database,
            &[batch_a.to_str().unwrap(), batch_b.to_str().unwrap()],
            &dog_typedb::QueryOptions { max_response_bytes:900, ..Default::default() }).await.unwrap_err();
        assert!(error.to_string().contains("Response byte limit"));
        assert!(adapter.write(json!({"query":"insert $x isa batch-a;"})).await.is_err());
        assert!(adapter.write(json!({"query":"insert $x isa batch-b;"})).await.is_err());

        // A schema lock exercises the whole-operation deadline and connection reuse.
        let held = driver
            .transaction(&database, typedb_driver::TransactionType::Schema)
            .await?;
        held.query("define entity held-lock;").await?;
        let deadline = dog_typedb::QueryOptions {
            timeout: std::time::Duration::from_millis(150),
            ..Default::default()
        };
        let start = std::time::Instant::now();
        assert!(dog_typedb::execute_query_with_options(
            &driver,
            &database,
            "define entity blocked-schema;",
            dog_typedb::TransactionType::Schema,
            &deadline
        )
        .await
        .is_err());
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        held.close().await?;
        adapter
            .schema(json!({"query":"define entity blocked-schema;"}))
            .await?;
        adapter
            .read(json!({"query":"match $x isa capacity-item;"}))
            .await?;
        assert!(adapter
            .write(json!({"query":"insert $x isa held-lock;"}))
            .await
            .is_err());

        Ok::<_, anyhow::Error>(())
    })
    .catch_unwind()
    .await;
    driver.databases().get(&database).await?.delete().await?;
    match result {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
