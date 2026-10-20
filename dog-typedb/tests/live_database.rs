//! Creates and deletes only a uniquely named test database on the configured server.
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

async fn test_driver() -> anyhow::Result<Arc<TypeDBDriver>> {
    let address = std::env::var("TYPEDB_ADDRESS")?;
    let local = ["127.0.0.1:", "localhost:", "[::1]:"]
        .iter()
        .any(|prefix| address.starts_with(prefix));
    let username = std::env::var("TYPEDB_USERNAME").or_else(|error| {
        if local {
            Ok("admin".into())
        } else {
            Err(error)
        }
    })?;
    let password = std::env::var("TYPEDB_PASSWORD").or_else(|error| {
        if local {
            Ok("password".into())
        } else {
            Err(error)
        }
    })?;
    // Remote credentials must never be transmitted over plaintext. Loopback keeps
    // the existing Docker/CI defaults; TLS can also be enabled there explicitly.
    let tls = match std::env::var("TYPEDB_TLS") {
        Ok(value) if value == "true" => true,
        Ok(value) if value == "false" && local => false,
        Ok(_) => anyhow::bail!("TYPEDB_TLS must be true (false is allowed only for loopback)"),
        Err(std::env::VarError::NotPresent) => !local,
        Err(error) => return Err(error.into()),
    };
    let driver = Arc::new(TypeDBDriverFactory::connect(&address, &username, &password, tls).await?);
    Ok(driver)
}

#[tokio::test]
#[ignore = "requires TypeDB credentials; disposable database only"]
async fn schema_policy_failure_preserves_private_cause_and_rolls_back() -> anyhow::Result<()> {
    use dog_typedb::isolation::{AuthenticatedScope, SchemaPolicyAdapter};
    let driver = test_driver().await?;
    let database = format!("dogrs-error-probe-{}", uuid::Uuid::new_v4().simple());
    driver.databases().create(&database).await?;
    let state = Arc::new(State {
        driver: driver.clone(),
        database: database.clone(),
    });
    let trusted = TypeDBAdapter::new(state.clone());
    let scoped = SchemaPolicyAdapter::new(state);
    let result = std::panic::AssertUnwindSafe(async {
        trusted.schema(json!({"query": include_str!("fixtures/schema_policy.tql")})).await?;
        trusted.write(json!({"query": "insert $m isa member, has tenant-key 'a', has actor-key 'alice', has active true, has member-role 'owner';"})).await?;
        let scope = AuthenticatedScope::new("a", "alice")?;
        let error = scoped.write(&scope, json!({"query": "insert $n isa note, has title 'private-rollback-probe'; update $n has tenant-key 'b'; fetch {'title':$n.title};"})).await.unwrap_err();
        let error = error.downcast_ref::<dog_core::DogError>().expect("Structured policy error");
        assert_eq!(error.code(), 500);
        assert!(error.source_ref().is_some(), "Underlying policy failure was discarded");
        let wire = serde_json::to_string(&error.sanitize_for_client())?;
        assert!(!wire.contains("private-rollback-probe"));
        assert!(!wire.contains("checkpoint"));
        let remaining = trusted.read(json!({"query":"match $n isa note, has title 'private-rollback-probe'; fetch {'title':$n.title};"})).await?;
        assert!(remaining["ok"]["answers"].as_array().unwrap().is_empty());
        Ok::<_, anyhow::Error>(())
    }).catch_unwind().await;
    driver.databases().get(&database).await?.delete().await?;
    match result {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Capability probe, NOT an isolation implementation. In particular, binding a
/// trusted tenant input does not force an untrusted query to use that input.
#[tokio::test]
#[ignore = "requires TypeDB credentials; creates and deletes one disposable database"]
async fn registry_free_security_capability_probe() -> anyhow::Result<()> {
    use futures::TryStreamExt;
    use typedb_driver::{answer::QueryAnswer, given::GivenRows, TransactionType};

    let driver = test_driver().await?;
    let database = format!("dogrs-security-probe-{}", uuid::Uuid::new_v4().simple());
    driver.databases().create(&database).await?;
    let adapter = TypeDBAdapter::new(Arc::new(State {
        driver: driver.clone(),
        database: database.clone(),
    }));
    let result = std::panic::AssertUnwindSafe(async {
        adapter.schema(json!({"query": r#"define
            attribute tenant-key, value string;
            entity probe-item @meta("dog.security.scope", "tenant"), owns tenant-key;
            fun probe_number() -> integer @meta("dog.security.context", "required"):
                match let $n = 7; return first $n;
        "#})).await?;
        adapter.write(json!({"query": r#"insert
            $a isa probe-item, has tenant-key "tenant-a";
            $b isa probe-item, has tenant-key "tenant-b";
        "#})).await?;
        let metadata = adapter.read(json!({"query": r#"match
            $t label probe-item;
            let $scope = get_meta("dog.security.scope", $t);
            let $context = get_fun_meta("dog.security.context", "probe_number");
            fetch { "scope": $scope, "context": $context };
        "#})).await?;
        assert_eq!(metadata["ok"]["answers"].as_array().unwrap().len(), 1);
        assert_eq!(metadata["ok"]["answers"][0]["data"]["scope"], "tenant");
        assert_eq!(metadata["ok"]["answers"][0]["data"]["context"], "required");

        let tx = driver.transaction(&database, TransactionType::Read).await?;
        for (query, expected) in [
            ("given $tenant: string; match $x isa probe-item, has tenant-key == $tenant;", 1),
            // Proof that immutable inputs alone do not establish a boundary.
            ("given $tenant: string; match $x isa probe-item;", 2),
        ] {
            let mut rows = GivenRows::new(vec!["tenant".into()], 1);
            rows.push_row(vec!["tenant-a".into()])?;
            let answer = tx.query_with_rows(query, rows).await?;
            let QueryAnswer::ConceptRowStream(_, stream) = answer else {
                anyhow::bail!("Expected row stream from capability probe");
            };
            assert_eq!(stream.try_collect::<Vec<_>>().await?.len(), expected);
        }
        let analysis = tx.analyze("match $x isa probe-item; fetch { 'nested': [match $y isa probe-item; fetch { 'id': iid($y) };] };").await?;
        assert!(analysis.fetch.is_some());
        tx.close().await?;

        // A zero-row final result does not prove a write had no effects.
        // A future enforcer must validate every stage, not just returned rows.
        let empty = adapter.write(json!({"query": r#"insert
            $x isa probe-item, has tenant-key "tenant-b";
            match $x has tenant-key "tenant-a";
        "#})).await?;
        assert!(empty["ok"]["answers"].as_array().unwrap().is_empty());
        let after = adapter.read(json!({"query": "match $x isa probe-item;"})).await?;
        assert_eq!(after["ok"]["answers"].as_array().unwrap().len(), 3);
        Ok::<_, anyhow::Error>(())
    }).catch_unwind().await;
    driver.databases().get(&database).await?.delete().await?;
    match result {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

#[tokio::test]
#[ignore = "requires TypeDB credentials; creates and deletes one disposable database"]
async fn registry_free_schema_policy_isolation() -> anyhow::Result<()> {
    use dog_typedb::isolation::{AuthenticatedScope, SchemaPolicyAdapter};
    let driver = test_driver().await?;
    let database = format!("dogrs-isolation-{}", uuid::Uuid::new_v4().simple());
    driver.databases().create(&database).await?;
    let state = Arc::new(State {
        driver: driver.clone(),
        database: database.clone(),
    });
    let trusted = TypeDBAdapter::new(state.clone());
    let scoped = Arc::new(SchemaPolicyAdapter::new(state));
    let result = std::panic::AssertUnwindSafe(async {
        trusted.schema(json!({"query": include_str!("fixtures/schema_policy.tql")})).await?;
        trusted.write(json!({"query": r#"insert
            $a isa note, has tenant-key "a", has title "a-private";
            $b isa note, has tenant-key "b", has title "b-private";
            $ma isa member, has tenant-key "a", has actor-key "alice", has active true, has member-role "owner";
            $mb isa member, has tenant-key "b", has actor-key "bob", has active true, has member-role "owner";
            $mc isa member, has tenant-key "a", has actor-key "carol", has active true, has member-role "reader";
            $secret isa secret, has title "credentials-never-public";
        "#})).await?;
        let a = AuthenticatedScope::new("a", "alice")?;
        let b = AuthenticatedScope::new("b", "bob")?;
        let function_attack = "match $x isa! note, has tenant-key 'a'; fetch { 'leak': [all_titles()] };";
        let control = trusted.read(json!({"query": function_attack})).await?;
        assert_eq!(control["ok"]["answers"][0]["data"]["leak"].as_array().unwrap().len(), 2);
        let safe_function = scoped.read(&a, json!({"query": function_attack})).await?;
        assert_eq!(safe_function["ok"]["answers"][0]["data"]["leak"].as_array().unwrap().len(), 1);
        let read = json!({"query": "match $x isa! note; fetch { 'title': $x.title };"});
        let (ra, rb) = tokio::join!(scoped.read(&a, read.clone()), scoped.read(&b, read.clone()));
        let (ra, rb) = (ra?, rb?);
        assert_eq!(ra["ok"]["answers"].as_array().unwrap().len(), 1);
        assert_eq!(ra["ok"]["answers"][0]["data"]["title"], "a-private");
        assert_eq!(rb["ok"]["answers"][0]["data"]["title"], "b-private");
        assert!(ra["ok"].get("query").is_none());
        // Caller filters do not replace the server-owned tenant constraint.
        let foreign = scoped.read(&a, json!({"query": "match $x isa! note, has tenant-key 'b'; fetch { 'title': $x.title };"})).await?;
        assert!(foreign["ok"]["answers"].as_array().unwrap().is_empty());
        let forged = AuthenticatedScope::new("b", "alice")?;
        assert!(scoped.read(&forged, read.clone()).await?["ok"]["answers"].as_array().unwrap().is_empty());
        assert!(scoped.read(&a, json!({"query": read["query"], "actor": "bob"})).await.is_err());
        for attack in [
            "match $x isa! secret; fetch { 'title': $x.title };",
            "match $x isa! member; fetch { 'actor': $x.actor-key };",
            "match $x isa! note; fetch { 'leak': stolen() };",
            "given $__dog_actor: string; match $x isa! note; fetch { 'title': $x.title };",
            "match $x isa! note; delete $x;",
            "define entity escape;",
        ] { assert!(scoped.read(&a, json!({"query": attack})).await.is_err(), "{attack}"); }

        let update = json!({"query": "match $x isa! note; update $x has title 'changed'; fetch { 'title': $x.title };"});
        assert!(scoped.read(&a, update.clone()).await.is_err());
        let carol = AuthenticatedScope::new("a", "carol")?;
        assert!(scoped.write(&carol, update.clone()).await?["ok"]["answers"].as_array().unwrap().is_empty());
        assert_eq!(scoped.read(&a, read.clone()).await?["ok"]["answers"][0]["data"]["title"], "a-private");
        let changed = scoped.write(&a, update).await?;
        assert_eq!(changed["ok"]["answers"][0]["data"]["title"], "changed");
        assert_eq!(scoped.read(&b, read.clone()).await?["ok"]["answers"][0]["data"]["title"], "b-private");
        let denied = scoped.write(&a, json!({"query": "match $x isa! note; update $x has title 'forbidden'; fetch { 'title': $x.title };"})).await?;
        assert!(denied["ok"]["answers"].as_array().unwrap().is_empty());
        assert!(scoped.write(&a, json!({"query": "match $x isa! note; update $x has tenant-key 'b'; fetch { 'title': $x.title };"})).await.is_err());
        assert!(scoped.write(&a, json!({"query": "match $x isa! note; update $x has tenant-alias 'b'; fetch { 'title': $x.title };"})).await.is_err());
        assert!(scoped.write(&a, json!({"query": "insert $x isa note; match $x has title 'absent';"})).await.is_err());
        assert_eq!(scoped.read(&a, read.clone()).await?["ok"]["answers"][0]["data"]["title"], "changed");
        exercise_schema_policy_pipeline(scoped.clone()).await?;
        // No shared mutable request context: interleaved requests keep their
        // own typed inputs even when they use the very same executor instance.
        let requests = (0..16).map(|index| {
            let adapter = scoped.clone();
            let query = read.clone();
            async move {
                let (tenant, actor, title) = if index % 2 == 0 { ("a", "alice", "changed") } else { ("b", "bob", "b-private") };
                let scope = AuthenticatedScope::new(tenant, actor)?;
                let response = adapter.read(&scope, query).await?;
                assert_eq!(response["ok"]["answers"][0]["data"]["title"], title);
                Ok::<_, anyhow::Error>(())
            }
        });
        for result in futures::future::join_all(requests).await { result?; }

        let foreign_iid = trusted.read(json!({"query": "match $x isa note, has tenant-key 'b'; fetch { 'id': iid($x) };"})).await?;
        let iid = foreign_iid["ok"]["answers"][0]["data"]["id"].as_str().unwrap();
        let by_iid = scoped.read(&a, json!({"query": format!("match $x isa! note, iid {iid}; fetch {{ 'title': $x.title }};")})).await?;
        assert!(by_iid["ok"]["answers"].as_array().unwrap().is_empty());

        // The full stream is collected and checked before committing. Exceeding
        // the budget must roll back ALL updates, not return a partial success.
        trusted.write(json!({"query": "insert $x isa note, has tenant-key 'a', has title 'second-a';"})).await?;
        let limited = SchemaPolicyAdapter::new(Arc::new(State { driver: driver.clone(), database: database.clone() }))
            .with_options(dog_typedb::QueryOptions { max_answers: 1, ..Default::default() })?;
        assert!(limited.write(&a, json!({"query": "match $x isa! note; update $x has title 'must-rollback'; fetch { 'title': $x.title };"})).await.is_err());
        let intact = scoped.read(&a, read.clone()).await?;
        let titles: std::collections::BTreeSet<_> = intact["ok"]["answers"].as_array().unwrap().iter().map(|r| r["data"]["title"].as_str().unwrap()).collect();
        assert_eq!(titles, ["changed", "second-a"].into_iter().collect());
        exercise_extended_schema_policy(scoped.clone(), &trusted).await?;
        // Revocation is evaluated by the schema function on the next request.
        trusted.write(json!({"query": "match $m isa member, has actor-key 'alice'; update $m has active false;"})).await?;
        assert!(scoped.read(&a, read).await?["ok"]["answers"].as_array().unwrap().is_empty());
        Ok::<_, anyhow::Error>(())
    }).catch_unwind().await;
    driver.databases().get(&database).await?.delete().await?;
    match result {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

async fn exercise_extended_schema_policy(
    adapter: Arc<dog_typedb::isolation::SchemaPolicyAdapter>,
    trusted: &TypeDBAdapter,
) -> anyhow::Result<()> {
    use anyhow::Context;
    use dog_typedb::isolation::AuthenticatedScope;
    let a = AuthenticatedScope::new("a", "alice")?;
    let b = AuthenticatedScope::new("b", "bob")?;
    let reader = AuthenticatedScope::new("a", "carol")?;
    trusted.write(json!({"query":"insert $a isa company, has tenant-key 'a'; $b isa company, has tenant-key 'b';"})).await?;
    let graph_query = "match $c isa! company; put $b isa book, has title 'graph-book'; (company:$c,book:$b) isa book-owner; fetch {'title':$b.title};";
    let graph_a = adapter
        .write(&a, json!({"query":graph_query}))
        .await
        .context("same-stage entity and ownership relation")?;
    assert_eq!(graph_a["ok"]["answers"][0]["data"]["title"], "graph-book");
    let graph_b = adapter
        .write(&b, json!({"query":graph_query}))
        .await
        .context("same title independent tenant ownership")?;
    assert_eq!(graph_b["ok"]["answers"].as_array().unwrap().len(), 1);
    assert_eq!(
        adapter.write(&a, json!({"query":graph_query})).await?["ok"]["answers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let books = adapter
        .read(
            &a,
            json!({"query":"match $b isa! book; fetch {'title':$b.title};"}),
        )
        .await?;
    assert_eq!(books["ok"]["answers"].as_array().unwrap().len(), 1);
    for (scope, query) in [
        (&reader, graph_query.replace("graph-book", "reader-graph")),
        (&a, graph_query.replace("graph-book", "forbidden")),
        (
            &a,
            "put $b isa book, has title 'orphan'; fetch {'title':$b.title};".into(),
        ),
    ] {
        assert!(adapter.write(scope, json!({"query":query})).await.is_err());
    }
    assert_eq!(
        trusted
            .read(json!({"query":"match $b isa book; fetch {'title':$b.title};"}))
            .await?["ok"]["answers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let actor = adapter
        .read(
            &a,
            json!({"query":"match let $seed=1; fetch {'actor':request_actor()};"}),
        )
        .await?;
    assert_eq!(actor["ok"]["answers"][0]["data"]["actor"], "alice");
    for query in [
        "match $n isa! note; fetch {'private':$n.private-value};",
        "match $n isa! note, has private-value $secret; fetch {'private':$secret};",
        "match $n isa! note, has private-sub $secret; fetch {'private':$secret};",
        "match let $seed=1; fetch {'actor':request_actor('b','bob',2026-01-01T00:00:00)};",
    ] {
        assert!(adapter.read(&a, json!({"query":query})).await.is_err());
    }
    let denied_chain = adapter.write(&reader, json!({"query":"insert $n isa note, has title 'reader-chain'; update $n has title 'reader-chain-done'; fetch {'title':$n.title};"})).await?;
    assert!(denied_chain["ok"]["answers"].as_array().unwrap().is_empty());
    assert_eq!(denied_chain["ok"]["queryType"], "write");
    assert_eq!(denied_chain["ok"]["answerType"], "conceptDocuments");
    adapter.write(&a, json!({"query":"insert $n isa note, has title 'fan-one'; insert $m isa note, has title 'fan-two'; fetch {'id':iid($n)};"})).await.context("two inserts with bound objects retained")?;
    let fan = adapter.write(&a, json!({"query":"match $n isa! note, has title $title; {$title == 'fan-one';} or {$title == 'fan-two';}; update $n has title 'fan-changed'; update $n has title 'fan-final'; fetch {'id':iid($n)};"})).await.context("multi-row mutation lineage")?;
    assert_eq!(fan["ok"]["answers"].as_array().unwrap().len(), 2);
    let id = fan["ok"]["answers"][0]["data"]["id"].as_str().unwrap();
    let partial = format!("match $n isa! note, has title 'fan-final'; let $id=iid($n); update $n has title 'fan-rollback'; match $id == {}; update $n has title 'fan-partial'; fetch {{'id':iid($n)}};", serde_json::to_string(id)?);
    assert!(adapter.write(&a, json!({"query":partial})).await.is_err());
    let unchanged = adapter
        .read(
            &a,
            json!({"query":"match $n isa! note, has title 'fan-final'; fetch {'id':iid($n)};"}),
        )
        .await?;
    assert_eq!(unchanged["ok"]["answers"].as_array().unwrap().len(), 2);
    adapter
        .write(
            &a,
            json!({"query":"match $n isa! note, has title 'fan-final'; delete $n;"}),
        )
        .await?;
    let chained = adapter.write(&a, json!({"query": "put $n isa note, has title 'checkpoint-new'; update $n has title 'checkpoint-updated'; fetch {'title': $n.title};"})).await.context("put then update in one transaction")?;
    assert_eq!(
        chained["ok"]["answers"][0]["data"]["title"],
        "checkpoint-updated"
    );
    let existing = adapter.write(&a, json!({"query": "put $n isa note, has title 'checkpoint-updated'; fetch {'title': $n.title};"})).await.context("put finds scoped existing object")?;
    assert_eq!(existing["ok"]["answers"].as_array().unwrap().len(), 1);
    for query in [
        "insert $n isa note, has title 'checkpoint-rollback'; update $n has title 'forbidden'; fetch {'title': $n.title};",
        "insert $n isa note, has title 'checkpoint-rollback'; match $n has title 'never'; update $n has title 'other'; fetch {'title': $n.title};",
        "insert $n isa note, has title 'checkpoint-rollback'; update $n has title 'unreadable'; fetch {'title': $n.title};",
        "insert $n isa note, has title 'checkpoint-rollback'; insert $other isa secret, has title 'private'; fetch {'title': $n.title};",
    ] {
        assert!(adapter.write(&a, json!({"query":query})).await.is_err());
    }
    assert!(trusted.read(json!({"query":"match $n isa note, has title 'checkpoint-rollback'; fetch {'id':iid($n)};"})).await?["ok"]["answers"].as_array().unwrap().is_empty());
    adapter
        .write(
            &a,
            json!({"query":"match $n isa! note, has title 'checkpoint-updated'; delete $n;"}),
        )
        .await?;
    let parallel = json!({"query": "insert $n isa note, has title 'concurrent'; fetch { 'tenant': $n.tenant-key };"});
    let (pa, pb) = tokio::join!(
        adapter.write(&a, parallel.clone()),
        adapter.write(&b, parallel)
    );
    assert_eq!(pa?["ok"]["answers"][0]["data"]["tenant"], "a");
    assert_eq!(pb?["ok"]["answers"][0]["data"]["tenant"], "b");
    let remove_parallel =
        json!({"query": "match $n isa! note, has title 'concurrent'; delete $n;"});
    let (pa, pb) = tokio::join!(
        adapter.write(&a, remove_parallel.clone()),
        adapter.write(&b, remove_parallel)
    );
    assert_eq!(pa?["ok"]["answers"].as_array().unwrap().len(), 1);
    assert_eq!(pb?["ok"]["answers"].as_array().unwrap().len(), 1);
    let atomic = adapter.write(&a, json!({"query": "insert $ok isa note, has title 'atomic-ok'; $denied isa note, has title 'forbidden'; fetch { 'title': $ok.title };"})).await?;
    assert!(atomic["ok"]["answers"].as_array().unwrap().is_empty());
    assert!(trusted
        .read(
            json!({"query": "match $n isa note, has title 'atomic-ok'; fetch { 'id': iid($n) };"})
        )
        .await?["ok"]["answers"]
        .as_array()
        .unwrap()
        .is_empty());
    let nested = adapter.read(&a, json!({"query": "match $outer isa! note; fetch { 'children': [match $inner isa! note; fetch { 'title': $inner.title };] };"})).await.context("scoped nested query")?;
    assert_eq!(nested["ok"]["answers"].as_array().unwrap().len(), 2);
    for row in nested["ok"]["answers"].as_array().unwrap() {
        let children = row["data"]["children"].as_array().unwrap();
        assert_eq!(children.len(), 2);
        assert!(children.iter().all(|c| c["title"] != "b-private"));
    }
    let functions = adapter.read(&a, json!({"query": "match let $seed=1; fetch { 'titles': [wrapped_titles()], 'count': count_notes() };"})).await.context("nested helper and count functions")?;
    assert_eq!(
        functions["ok"]["answers"][0]["data"]["count"].as_f64(),
        Some(2.0)
    );
    assert_eq!(
        functions["ok"]["answers"][0]["data"]["titles"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let constant = adapter
        .read(
            &a,
            json!({"query": "match let $seed=1; fetch { 'four': four() };"}),
        )
        .await
        .context("function without data access")?;
    assert_eq!(
        constant["ok"]["answers"][0]["data"]["four"].as_f64(),
        Some(4.0)
    );
    let objects = adapter
        .read(
            &a,
            json!({"query": "match let $n in note_objects(); fetch { 'title': $n.title };"}),
        )
        .await
        .context("function returning objects")?;
    assert_eq!(objects["ok"]["answers"].as_array().unwrap().len(), 2);
    assert!(adapter
        .read(
            &a,
            json!({"query": "match let $seed=1; fetch { 'secret': [secret_titles()] };"})
        )
        .await
        .is_err());
    assert!(adapter.read(&a, json!({"query": "match $n isa! note; fetch { 'bad': [match $x isa! note; delete $x;] };"})).await.is_err());
    assert!(adapter.read(&a, json!({"query": "with fun all_titles() -> { string }: match let $x='forged'; return { $x }; fetch { 'v': [all_titles()] };"})).await.is_err());

    let invalid_policy = adapter
        .read(
            &a,
            json!({"query": "match $n isa! unsafe-note; fetch { 'tenant': $n.tenant-key };"}),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(!invalid_policy.contains("unsafe_read") && !invalid_policy.contains("tenant-key"));
    let create = json!({"query": "insert $new isa note, has title 'created'; fetch { 'title': $new.title };"});
    assert!(adapter
        .write(&reader, create.clone())
        .await
        .context("reader insert denied")?["ok"]["answers"]
        .as_array()
        .unwrap()
        .is_empty());
    let created = adapter
        .write(&a, create)
        .await
        .context("authorized insert")?;
    assert_eq!(created["ok"]["answers"][0]["data"]["title"], "created");
    let forbidden = adapter.write(&a, json!({"query": "insert $n isa note, has title 'forbidden'; fetch { 'title': $n.title };"})).await?;
    assert!(forbidden["ok"]["answers"].as_array().unwrap().is_empty());
    assert!(adapter
        .write(
            &a,
            json!({"query": "insert $n isa note, has title 'unreadable'; fetch { 'ok': true };"})
        )
        .await
        .is_err());
    assert!(adapter.write(&a, json!({"query": "insert $n isa note, has title 'unreadable'; fetch { 'value': empty_value() };"})).await.is_err());
    let unreadable = trusted.read(json!({"query": "match $n isa note, has title 'unreadable'; fetch { 'title': $n.title };"})).await?;
    assert!(
        unreadable["ok"]["answers"].as_array().unwrap().is_empty(),
        "post-insert denial must roll back"
    );
    for attack in [
        "insert $n isa note, has title 'x', has tenant-key 'b'; fetch { 'title': $n.title };",
        "insert $n isa note, has title 'x', has tenant-alias 'b'; fetch { 'title': $n.title };",
        "insert $n isa note, has title 'x', has active true; fetch { 'title': $n.title };",
        "insert $n isa secret, has title 'x'; fetch { 'title': $n.title };",
    ] {
        assert!(
            adapter.write(&a, json!({"query": attack})).await.is_err(),
            "{attack}"
        );
    }

    let link = adapter.write(&a, json!({"query": "match $l isa! note, has title 'created'; $r isa! note, has title 'changed'; insert $edge isa note-link, links (left: $l, right: $r); fetch { 'id': iid($edge) };"})).await.context("authorized relation insert")?;
    assert_eq!(link["ok"]["answers"].as_array().unwrap().len(), 1);
    let traversal = "match $edge isa! note-link, links (left: $l, right: $r); $l isa! note; $r isa! note; fetch { 'left': $l.title, 'right': $r.title };";
    let rows = adapter
        .read(&a, json!({"query": traversal}))
        .await
        .context("relation traversal")?;
    assert_eq!(rows["ok"]["answers"][0]["data"]["left"], "created");
    assert_eq!(rows["ok"]["answers"][0]["data"]["right"], "changed");
    assert!(
        adapter.read(&b, json!({"query": traversal})).await?["ok"]["answers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let foreign = adapter.write(&a, json!({"query": "match $l isa! note, has title 'created'; $r isa! note, has title 'b-private'; insert $edge isa note-link, links (left: $l, right: $r); fetch { 'id': iid($edge) };"})).await?;
    assert!(foreign["ok"]["answers"].as_array().unwrap().is_empty());
    // A legacy corrupt cross-tenant edge must not be traversed or silently
    // removed through deleting an otherwise authorized participant.
    trusted.write(json!({"query": "match $l isa note, has title 'created'; $r isa note, has title 'b-private'; insert $edge isa note-link, has tenant-key 'a', links (left: $l, right: $r);"})).await?;
    let rows = adapter.read(&a, json!({"query": traversal})).await?;
    assert_eq!(rows["ok"]["answers"].as_array().unwrap().len(), 1);
    let remove_note = json!({"query": "match $n isa! note, has title 'created'; delete $n;"});
    let function_links = adapter
        .read(
            &a,
            json!({"query": "match let $seed=1; fetch { 'titles': [linked_titles()] };"}),
        )
        .await
        .context("anonymous relation in helper function")?;
    assert_eq!(
        function_links["ok"]["answers"][0]["data"]["titles"],
        json!(["changed"])
    );
    assert!(adapter.read(&a, json!({"query": "match $_ isa! note; let $x = $dog_security_generated_0; fetch { 'x': $x };"})).await.is_err());
    assert!(adapter
        .write(&a, remove_note.clone())
        .await
        .context("cascade refusal")?["ok"]["answers"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(adapter
        .write(
            &reader,
            json!({"query": "match $n isa! note, has title 'second-a'; delete $n;"})
        )
        .await?["ok"]["answers"]
        .as_array()
        .unwrap()
        .is_empty());
    let delete_link = "match $edge isa! note-link, links (left: $l, right: $r); $l isa! note; $r isa! note; delete $edge;";
    trusted.write(json!({"query": "match $edge isa note-link, links (right: $r); $r has title 'changed'; insert $h isa holder, has tenant-key 'b', links (child: $edge);"})).await?;
    let blocked = adapter
        .write(&a, json!({"query": delete_link}))
        .await
        .context("relation participating in a foreign parent")?;
    assert!(blocked["ok"]["answers"].as_array().unwrap().is_empty());
    let parent = trusted
        .read(json!({"query": "match $h isa holder; fetch { 'id': iid($h) };"}))
        .await?;
    assert_eq!(parent["ok"]["answers"].as_array().unwrap().len(), 1);
    trusted
        .write(json!({"query": "match $h isa holder; delete $h;"}))
        .await?;
    let removed = adapter
        .write(&a, json!({"query": delete_link}))
        .await
        .context("authorized relation deletion")?;
    assert_eq!(removed["ok"]["answers"].as_array().unwrap().len(), 1);
    let remaining = trusted
        .read(json!({"query": "match $r isa note-link; fetch { 'id': iid($r) };"}))
        .await?;
    assert_eq!(
        remaining["ok"]["answers"].as_array().unwrap().len(),
        1,
        "foreign incident relation must remain"
    );
    let unbound_delete = adapter
        .write(&a, json!({"query": "match $r isa! note-link; delete $r;"}))
        .await
        .context("unknown role-player refusal")?;
    assert!(unbound_delete["ok"]["answers"]
        .as_array()
        .unwrap()
        .is_empty());
    // Fixture cleanup is trusted, explicit and confined to this temporary DB.
    trusted
        .write(json!({"query": "match $r isa note-link; delete $r;"}))
        .await?;
    assert_eq!(
        adapter
            .write(&a, remove_note)
            .await
            .context("authorized entity deletion")?["ok"]["answers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let counts = adapter
        .read(
            &a,
            json!({"query": "match let $seed=1; fetch { 'count': count_notes() };"}),
        )
        .await?;
    assert_eq!(
        counts["ok"]["answers"][0]["data"]["count"].as_f64(),
        Some(2.0)
    );
    let counts = adapter
        .read(
            &b,
            json!({"query": "match let $seed=1; fetch { 'count': count_notes() };"}),
        )
        .await?;
    assert_eq!(
        counts["ok"]["answers"][0]["data"]["count"].as_f64(),
        Some(1.0)
    );
    Ok(())
}

/// JSON transport request -> authentication hook -> thin custom read/write
/// service -> registry-free executor -> real TypeDB policies. Tokens here are
/// fixture credentials, not an application JWT implementation.
async fn exercise_schema_policy_pipeline(
    adapter: Arc<dog_typedb::isolation::SchemaPolicyAdapter>,
) -> anyhow::Result<()> {
    use async_trait::async_trait;
    use dog_core::{
        DogAppBuilder, DogBeforeHook, DogService, HookContext, ServiceCapabilities,
        ServiceMethodKind, TenantContext,
    };
    use dog_typedb::isolation::{AuthenticatedScope, SchemaPolicyAdapter};
    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    #[derive(Clone, Default, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Params {
        token: Option<String>,
        provider: Option<String>,
        #[serde(default)]
        headers: std::collections::HashMap<String, String>,
        #[serde(default)]
        query: std::collections::HashMap<String, String>,
        method: Option<String>,
        path: Option<String>,
        raw_query: Option<String>,
        inner: Option<Value>,
        #[serde(skip)]
        scope: Option<Arc<AuthenticatedScope>>,
    }
    struct Authenticate;
    #[async_trait]
    impl DogBeforeHook<Value, Params> for Authenticate {
        async fn run(&self, ctx: &mut HookContext<Value, Params>) -> anyhow::Result<()> {
            ctx.params.scope = None;
            let token = ctx.params.token.as_deref().or_else(|| {
                ctx.params
                    .headers
                    .get("authorization")
                    .and_then(|s| s.strip_prefix("Bearer "))
            });
            let (tenant, actor) = match token {
                Some("fixture-token-a") => ("a", "alice"),
                Some("fixture-token-b") => ("b", "bob"),
                Some("fixture-token-reader") => ("a", "carol"),
                _ => {
                    return Err(
                        dog_core::DogError::not_authenticated("Unauthenticated").into_anyhow()
                    )
                }
            };
            if ctx.tenant.tenant_id.0 != tenant {
                return Err(dog_core::DogError::forbidden("Tenant mismatch").into_anyhow());
            }
            ctx.params.scope = Some(Arc::new(AuthenticatedScope::new(tenant, actor)?));
            Ok(())
        }
    }
    struct DataService(Arc<SchemaPolicyAdapter>);
    #[async_trait]
    impl DogService<Value, Params> for DataService {
        fn capabilities(&self) -> ServiceCapabilities {
            ServiceCapabilities::from_methods(vec![
                ServiceMethodKind::Custom("read"),
                ServiceMethodKind::Custom("write"),
            ])
        }
        async fn custom(
            &self,
            _: &TenantContext,
            method: &str,
            data: Option<Value>,
            params: Params,
        ) -> anyhow::Result<Value> {
            let scope = params
                .scope
                .ok_or_else(|| anyhow::anyhow!("Missing authenticated scope"))?;
            let data = data.ok_or_else(|| anyhow::anyhow!("Missing request body"))?;
            match method {
                "read" => self.0.read(&scope, data).await,
                "write" => self.0.write(&scope, data).await,
                _ => anyhow::bail!("Method not allowed"),
            }
        }
    }
    let mut builder = DogAppBuilder::<Value, Params>::new();
    builder.register_service("data", Arc::new(DataService(adapter)));
    builder.service_hooks("data", |hooks| {
        hooks.before(ServiceMethodKind::Custom("read"), Arc::new(Authenticate));
        hooks.before(ServiceMethodKind::Custom("write"), Arc::new(Authenticate));
    });
    let app = builder.build();
    // Actual loopback HTTP, using the framework transport rather than a fake
    // endpoint that calls the adapter directly. No production listener is used.
    {
        use dog_transport::{HttpOptions, IntoDogService};
        struct Server(tokio::task::JoinHandle<()>);
        impl Drop for Server {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = axum::Router::new().fallback_service(
            app.clone()
                .into_service(HttpOptions::new().body_limit(32 * 1024)),
        );
        let _server = Server(tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("HTTP fixture server");
        }));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        let endpoint = format!("http://{address}/data");
        let body = json!({"query": "match $x isa! note; fetch { 'titles': [wrapped_titles()], 'title': $x.title };"});
        let (a, b) = tokio::join!(
            client
                .post(&endpoint)
                .header("x-tenant-id", "a")
                .header("x-service-method", "read")
                .bearer_auth("fixture-token-a")
                .json(&body)
                .send(),
            client
                .post(&endpoint)
                .header("x-tenant-id", "b")
                .header("x-service-method", "read")
                .bearer_auth("fixture-token-b")
                .json(&body)
                .send()
        );
        let a = a?.error_for_status()?.json::<Value>().await?;
        let b = b?.error_for_status()?.json::<Value>().await?;
        assert_eq!(a["ok"]["answers"][0]["data"]["title"], "changed");
        assert_eq!(b["ok"]["answers"][0]["data"]["title"], "b-private");
        let no_token = client
            .post(&endpoint)
            .header("x-tenant-id", "a")
            .header("x-service-method", "read")
            .json(&body)
            .send()
            .await?;
        assert_eq!(no_token.status().as_u16(), 401);
        let wrong_tenant = client
            .post(&endpoint)
            .header("x-tenant-id", "b")
            .header("x-service-method", "read")
            .bearer_auth("fixture-token-a")
            .json(&body)
            .send()
            .await?;
        assert_eq!(wrong_tenant.status().as_u16(), 403);
        let denied_write = client.post(&endpoint).header("x-tenant-id", "a").header("x-service-method", "write").bearer_auth("fixture-token-reader")
            .json(&json!({"query": "insert $n isa note, has title 'http-forgery'; fetch { 'title': $n.title };"})).send().await?.error_for_status()?.json::<Value>().await?;
        assert!(denied_write["ok"]["answers"].as_array().unwrap().is_empty());
        let forged = client
            .post(&endpoint)
            .header("x-tenant-id", "a")
            .header("x-service-method", "read")
            .bearer_auth("fixture-token-a")
            .json(&json!({"query": body["query"], "tenant": "b", "actor": "bob"}))
            .send()
            .await?;
        assert!(!forged.status().is_success());
        let created = client.post(&endpoint).header("x-tenant-id", "a").header("x-service-method", "write").bearer_auth("fixture-token-a")
            .json(&json!({"query": "insert $n isa note, has title 'http-created'; fetch { 'title': $n.title };"})).send().await?.error_for_status()?.json::<Value>().await?;
        assert_eq!(created["ok"]["answers"][0]["data"]["title"], "http-created");
        let foreign = client.post(&endpoint).header("x-tenant-id", "b").header("x-service-method", "read").bearer_auth("fixture-token-b")
            .json(&json!({"query": "match $n isa! note, has title 'http-created'; fetch { 'title': $n.title };"})).send().await?.error_for_status()?.json::<Value>().await?;
        assert!(foreign["ok"]["answers"].as_array().unwrap().is_empty());
        let deleted = client
            .post(&endpoint)
            .header("x-tenant-id", "a")
            .header("x-service-method", "write")
            .bearer_auth("fixture-token-a")
            .json(&json!({"query": "match $n isa! note, has title 'http-created'; delete $n;"}))
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        assert_eq!(deleted["ok"]["answers"].as_array().unwrap().len(), 1);
        let chained = client.post(&endpoint).header("x-tenant-id", "a")
            .header("x-service-method", "write").bearer_auth("fixture-token-a")
            .json(&json!({"query":"put $n isa note, has title 'http-chain'; update $n has title 'http-chain-done'; fetch {'title':$n.title};"}))
            .send().await?.error_for_status()?.json::<Value>().await?;
        assert_eq!(
            chained["ok"]["answers"][0]["data"]["title"],
            "http-chain-done"
        );
        let denied = client.post(&endpoint).header("x-tenant-id", "a")
            .header("x-service-method", "write").bearer_auth("fixture-token-a")
            .json(&json!({"query":"match $n isa! note, has title 'http-chain-done'; update $n has title 'http-chain-rollback'; update $n has title 'forbidden'; fetch {'title':$n.title};"}))
            .send().await?;
        assert!(!denied.status().is_success());
        let still_present = client.post(&endpoint).header("x-tenant-id", "a")
            .header("x-service-method", "read").bearer_auth("fixture-token-a")
            .json(&json!({"query":"match $n isa! note, has title 'http-chain-done'; fetch {'title':$n.title};"}))
            .send().await?.error_for_status()?.json::<Value>().await?;
        assert_eq!(still_present["ok"]["answers"].as_array().unwrap().len(), 1);
        client
            .post(&endpoint)
            .header("x-tenant-id", "a")
            .header("x-service-method", "write")
            .bearer_auth("fixture-token-a")
            .json(&json!({"query":"match $n isa! note, has title 'http-chain-done'; delete $n;"}))
            .send()
            .await?
            .error_for_status()?;
    }
    let request =
        |tenant: &str, token: Option<&str>, method: &str, payload: Value| -> dog_core::DogRequest {
            // Exercise the same deserialization boundary as a JSON transport.
            serde_json::from_value(json!({
                "request_id": null, "transport": "Http", "service": "data",
                "method": {"Custom": method}, "id": null,
                "tenant": {"tenant_id": tenant}, "params": {"token": token},
                "payload": payload, "metadata": {}
            }))
            .unwrap()
        };
    let read = json!({"query": "match $x isa! note; fetch { 'title': $x.title };"});
    let (a, b) = tokio::join!(
        app.handle(request("a", Some("fixture-token-a"), "read", read.clone())),
        app.handle(request("b", Some("fixture-token-b"), "read", read.clone()))
    );
    assert_eq!(
        a?.payload.unwrap()["ok"]["answers"][0]["data"]["title"],
        "changed"
    );
    assert_eq!(
        b?.payload.unwrap()["ok"]["answers"][0]["data"]["title"],
        "b-private"
    );
    for (tenant, token) in [
        ("a", None),
        ("a", Some("invalid")),
        ("b", Some("fixture-token-a")),
    ] {
        assert!(app
            .handle(request(tenant, token, "read", read.clone()))
            .await
            .is_err());
    }
    let mut forged = request("a", Some("fixture-token-a"), "read", read.clone());
    forged.params.inner.insert("actor".into(), json!("bob"));
    assert!(app.handle(forged).await.is_err());
    assert!(app
        .service("data")?
        .custom(
            TenantContext::new("a"),
            "read",
            Some(read.clone()),
            Params {
                scope: Some(Arc::new(AuthenticatedScope::new("b", "bob")?)),
                ..Default::default()
            }
        )
        .await
        .is_err());
    let denied = app.handle(request("a", Some("fixture-token-reader"), "write", json!({"query": "match $x isa! note; update $x has title 'reader-hacked'; fetch { 'title': $x.title };"}))).await?;
    assert!(denied.payload.unwrap()["ok"]["answers"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(app
        .handle(request(
            "a",
            Some("fixture-token-a"),
            "schema",
            json!({"query": "define entity escape;"})
        ))
        .await
        .is_err());
    Ok(())
}

#[tokio::test]
#[ignore = "requires TypeDB credentials and permission to create a test database; run with --ignored"]
async fn transaction_boundaries_and_atomic_schema_loading() -> anyhow::Result<()> {
    let driver = test_driver().await?;
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

#[tokio::test]
#[ignore = "creates/deletes only a unique tenant-security fixture database; requires TypeDB"]
async fn scoped_adapter_enforces_tenants_and_rolls_back_invalid_results() -> anyhow::Result<()> {
    use dog_core::tenant::TenantContext;
    use dog_typedb::{QueryContract, QueryMethod, QueryParameter, TenantQueryPolicy};
    let driver = test_driver().await?;
    let database = format!("dogrs-tenant-test-{}", uuid::Uuid::new_v4().simple());
    driver.databases().create(&database).await?;
    let result=std::panic::AssertUnwindSafe(async {
        let state=Arc::new(State {driver:driver.clone(),database:database.clone()});
        let raw=TypeDBAdapter::new(state.clone());
        raw.schema(json!({"query":r#"define
            attribute tenant-id, value string;
            attribute subject-id, value string;
            attribute content, value string;
            attribute access, value string @values("read","write");
            entity tenant-member, owns tenant-id @card(1), owns subject-id @key, owns access @card(1);
            entity tenant-record, owns tenant-id @card(1), owns content @card(1);
            fun permitted($tenant:string,$subject:string,$mode:string)->boolean:
                match $m isa tenant-member,has tenant-id == $tenant,has subject-id == $subject,has access $level;
                {$mode=="read";} or {$mode=="write";$level=="write";};
                return check;
            fun visible($tenant:string,$subject:string)->{tenant-record}:
                match let $ok=permitted($tenant,$subject,"read");$ok==true;
                $r isa tenant-record,has tenant-id == $tenant;
                return {$r};
        "#})).await?;
        raw.write(json!({"query":r#"insert
            $a isa tenant-member,has tenant-id "a",has subject-id "alice",has access "write";
            $b isa tenant-member,has tenant-id "b",has subject-id "bob",has access "write";
            $c isa tenant-member,has tenant-id "a",has subject-id "carol",has access "read";
            $x isa tenant-record,has tenant-id "a",has content "A";
            $y isa tenant-record,has tenant-id "b",has content "B";
        "#})).await?;
        let read=r#"match let $r in visible(@@tenant_id@@,@@identity_id@@);fetch {"content":$r.content};"#;
        let write=r#"match let $ok=permitted(@@tenant_id@@,@@identity_id@@,"write");$ok==true;insert $r isa tenant-record,has tenant-id @@tenant_id@@,has content @@content@@;fetch {"content":$r.content};"#;
        // Deliberately reviewed bulk-write fixture with a single-result contract,
        // proving result validation occurs inside the transaction before commit.
        let bulk=r#"match let $ok=permitted(@@tenant_id@@,@@identity_id@@,"write");$ok==true;$old isa tenant-record,has tenant-id @@tenant_id@@;insert $r isa tenant-record,has tenant-id @@tenant_id@@,has content "must-rollback";fetch {"content":$r.content};"#;
        let policy=TenantQueryPolicy::new([
            QueryContract::new(QueryMethod::Read,read),
            QueryContract::new(QueryMethod::Write,write).parameter("content",QueryParameter::String {max_bytes:1024}).one_answer(),
            QueryContract::new(QueryMethod::Write,bulk).one_answer(),
        ])?;
        let alice=policy.authenticated_context(&TenantContext::new("a"),"alice",[])?;
        let bob=policy.authenticated_context(&TenantContext::new("b"),"bob",[])?;
        let carol=policy.authenticated_context(&TenantContext::new("a"),"carol",[])?;
        let scoped=TypeDBAdapter::new(state).tenant_scoped(policy.clone());
        let render=|source:&str,tenant:&str,subject:&str| source
            .replace("@@tenant_id@@",&serde_json::to_string(tenant).unwrap())
            .replace("@@identity_id@@",&serde_json::to_string(subject).unwrap());
        let qa=render(read,"a","alice");
        let qb=render(read,"b","bob");
        // Request-local contexts must not contaminate each other under concurrency.
        let (a,b)=tokio::join!(
            scoped.read(&alice,json!({"query":qa})),
            scoped.read(&bob,json!({"query":qb})),
        );
        let a=a?;let b=b?;
        assert_eq!(a["ok"]["answers"][0]["data"]["content"],"A");
        assert_eq!(b["ok"]["answers"][0]["data"]["content"],"B");
        assert!(a.pointer("/ok/query").is_none());
        for forged in [
            qb.clone(),
            render(read,"a","carol"),
            "match $r isa tenant-record;fetch {\"content\":$r.content};".into(),
            format!("{qa} match $m isa tenant-member;"),
            qa.replace("visible(","unrestricted("),
        ] { assert!(scoped.read(&alice,json!({"query":forged})).await.is_err()); }
        assert!(scoped.read(&alice,json!({"query":qa,"tenant_id":"b"})).await.is_err());
        let injected=r#""; match $r isa tenant-record; delete $r; # @@tenant_id@@"#;
        let qw=render(write,"a","alice").replace("@@content@@",&serde_json::to_string(injected)?);
        let written=scoped.write(&alice,json!({"query":qw})).await?;
        assert_eq!(written["ok"]["answers"][0]["data"]["content"],injected);
        let qc=render(write,"a","carol").replace("@@content@@","\"denied\"");
        assert!(scoped.write(&carol,json!({"query":qc})).await.is_err());
        assert!(scoped.read(&alice,json!({"query":qw})).await.is_err());
        let qbulk=render(bulk,"a","alice");
        assert!(scoped.write(&alice,json!({"query":qbulk})).await.is_err());
        let remaining=raw.read(json!({"query":"match $r isa tenant-record;fetch {\"content\":$r.content};"})).await?;
        assert_eq!(remaining["ok"]["answers"].as_array().unwrap().len(),3);
        assert!(!remaining["ok"]["answers"].as_array().unwrap().iter()
            .any(|r|r["data"]["content"]=="must-rollback"||r["data"]["content"]=="denied"));
        // Membership removal takes effect through TypeDB policy, even when an
        // earlier request-local context still exists.
        raw.write(json!({"query":"match $m isa tenant-member,has subject-id \"alice\";delete $m;"})).await?;
        let revoked=scoped.read(&alice,json!({"query":qa})).await?;
        assert!(revoked["ok"]["answers"].as_array().unwrap().is_empty());
        assert!(scoped.write(&alice,json!({"query":qw})).await.is_err());
        Ok::<_,anyhow::Error>(())
    }).catch_unwind().await;
    // Never accept a caller-supplied cleanup target.
    assert!(database
        .strip_prefix("dogrs-tenant-test-")
        .is_some_and(|suffix| uuid::Uuid::parse_str(suffix).is_ok()));
    driver.databases().get(&database).await?.delete().await?;
    match result {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
