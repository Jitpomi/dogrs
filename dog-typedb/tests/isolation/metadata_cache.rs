use super::SchemaMetadata;
use serde_json::json;

#[test]
fn checkpoints_share_metadata_but_new_transactions_do_not() {
    let first = SchemaMetadata::default();
    let checkpoint = first.clone();
    let rows = vec![json!({"data":{"f":"scope_note"}})];
    first.insert("schema-only", &rows, 1024).unwrap();
    assert_eq!(checkpoint.get("schema-only").unwrap(), Some(rows));
    assert!(SchemaMetadata::default()
        .get("schema-only")
        .unwrap()
        .is_none());
    assert!(first.get("different-schema-question").unwrap().is_none());
}

#[test]
fn metadata_is_bounded_immutable_and_negative_results_are_cached() {
    let cache = SchemaMetadata::default();
    cache.insert("missing", &[], 1024).unwrap();
    assert_eq!(cache.get("missing").unwrap(), Some(vec![]));
    cache.insert("missing", &[], 1024).unwrap();
    assert!(cache.insert("missing", &[json!("changed")], 1024).is_err());
    assert!(cache
        .insert("too-large", &[json!("x".repeat(2048))], 1024)
        .is_err());
    assert!(cache.get("too-large").unwrap().is_none());
}

#[tokio::test]
#[ignore = "Real TypeDB; disposable schema-only database; metadata round-trip regression"]
async fn real_compiler_reuses_schema_reads_only_within_one_transaction() -> anyhow::Result<()> {
    use super::Compiler;
    use futures::FutureExt;
    use typedb_driver::TransactionType;
    let driver = dog_typedb_driver_for_test().await?;
    let database = format!("dogrs-metadata-probe-{}", uuid::Uuid::new_v4().simple());
    driver.databases().create(&database).await?;
    let result = std::panic::AssertUnwindSafe(async {
        let schema = include_str!("../fixtures/schema_policy.tql");
        let tx = driver
            .transaction(&database, TransactionType::Schema)
            .await?;
        tx.query(schema).await?;
        tx.commit().await?;
        let parsed = typeql::parse_query("match $n isa! note; fetch {'title':$n.title};")?;
        let typeql::query::QueryStructure::Pipeline(pipeline) = parsed.structure else {
            unreachable!()
        };
        let options = crate::QueryOptions::default();
        let metadata = SchemaMetadata::default();
        let tx = driver.transaction(&database, TransactionType::Read).await?;
        let first = Compiler::new(&tx, &options, schema)?
            .with_metadata(metadata.clone())
            .compile(&pipeline.stages, false)
            .await?;
        let fetches = metadata.0.lock().unwrap().fetches;
        assert!(fetches > 0);
        let second = Compiler::new(&tx, &options, schema)?
            .with_metadata(metadata.clone())
            .compile(&pipeline.stages, false)
            .await?;
        assert_eq!(
            first.source, second.source,
            "Metadata reuse changed compiled authorization"
        );
        assert_eq!(
            metadata.0.lock().unwrap().fetches,
            fetches,
            "Repeated schema introspection still hits TypeDB"
        );
        tx.close().await?;
        let tx = driver.transaction(&database, TransactionType::Read).await?;
        let fresh = SchemaMetadata::default();
        Compiler::new(&tx, &options, schema)?
            .with_metadata(fresh.clone())
            .compile(&pipeline.stages, false)
            .await?;
        assert_eq!(
            fresh.0.lock().unwrap().fetches,
            fetches,
            "New transaction reused another transaction's metadata"
        );
        tx.close().await?;
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

async fn dog_typedb_driver_for_test() -> anyhow::Result<typedb_driver::TypeDBDriver> {
    crate::TypeDBDriverFactory::connect(
        &std::env::var("TYPEDB_ADDRESS")?,
        &std::env::var("TYPEDB_USERNAME")?,
        &std::env::var("TYPEDB_PASSWORD")?,
        true,
    )
    .await
}
