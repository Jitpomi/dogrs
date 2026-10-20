//! Registry-free schema-driven enforcement for frontend TypeQL.
//! Unsupported constructs fail closed; no raw execution fallback is provided.
//! Remains experimental until the consuming app passes its migration tests.
use anyhow::{ensure, Context, Result};
use futures::StreamExt;
use serde_json::Value;
use std::sync::Arc;
use typedb_driver::{given::GivenRows, Transaction, TypeDBDriver};
use typeql::{expression::Expression, query::QueryStructure, TypeRef, Variable};
mod compiler;

#[derive(Default)]
struct PendingTransaction(Option<Transaction>);
impl Drop for PendingTransaction {
    fn drop(&mut self) {
        if let (Some(tx), Ok(runtime)) = (self.0.take(), tokio::runtime::Handle::try_current()) {
            // The caller/transport can cancel execute itself, not just its
            // internal deadline. Finish close independently of that request.
            runtime.spawn(async move {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), tx.close()).await;
            });
        }
    }
}

/// Server-owned request identity, never deserialized from JSON.
/// Authentication hooks must verify tokens and tenant membership before creation.
pub struct AuthenticatedScope {
    tenant: String,
    actor: String,
}
impl AuthenticatedScope {
    pub fn new(tenant: impl Into<String>, actor: impl Into<String>) -> Result<Self> {
        let (tenant, actor) = (tenant.into(), actor.into());
        ensure!(
            !tenant.is_empty() && !actor.is_empty(),
            "Missing authenticated scope"
        );
        ensure!(
            tenant.len() <= 1024 && actor.len() <= 1024,
            "Scope exceeds limit"
        );
        Ok(Self { tenant, actor })
    }
}
impl std::fmt::Debug for AuthenticatedScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthenticatedScope").finish_non_exhaustive()
    }
}

/// Opt-in executor; custom services retain read/write. No schema/raw escape.
pub struct SchemaPolicyAdapter {
    driver: Arc<TypeDBDriver>,
    database: String,
    options: crate::QueryOptions,
}
impl SchemaPolicyAdapter {
    pub fn new<T: crate::adapter::TypeDBState>(state: Arc<T>) -> Self {
        Self {
            driver: state.driver().clone(),
            database: state.database().into(),
            options: Default::default(),
        }
    }
    pub fn with_options(mut self, options: crate::QueryOptions) -> Result<Self> {
        options.validate()?;
        self.options = options;
        Ok(self)
    }
    pub async fn read(&self, scope: &AuthenticatedScope, data: Value) -> Result<Value> {
        self.execute(scope, data, false).await
    }
    pub async fn write(&self, scope: &AuthenticatedScope, data: Value) -> Result<Value> {
        self.execute(scope, data, true).await
    }
    async fn execute(&self, scope: &AuthenticatedScope, data: Value, write: bool) -> Result<Value> {
        let object = data
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Expected query object"))?;
        ensure!(
            object.len() == 1,
            "Only query is accepted; authentication is server-owned"
        );
        let query = object
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Missing query"))?;
        self.options.validate_query(query)?;
        ensure!(
            query.len() <= 32 * 1024,
            "Schema-policy query limit exceeded"
        );
        parser_budget(query)?;
        let parsed = typeql::parse_query(query).map_err(|_| anyhow::anyhow!("Invalid TypeQL"))?;
        let QueryStructure::Pipeline(pipeline) = parsed.structure else {
            anyhow::bail!("Schema access is forbidden")
        };
        ensure!(
            pipeline.preambles.is_empty(),
            "Caller-supplied function definitions are forbidden"
        );
        let mut committing = false;
        let mut phase = "opening transaction";
        let mut transaction = PendingTransaction::default();
        let operation = async {
            let mode = if write {
                typedb_driver::TransactionType::Write
            } else {
                typedb_driver::TransactionType::Read
            };
            transaction.0 = Some(
                self.driver
                    .transaction_with_options(
                        &self.database,
                        mode,
                        typedb_driver::TransactionOptions::new()
                            .transaction_timeout(self.options.timeout)
                            .schema_lock_acquire_timeout(self.options.timeout),
                    )
                    .await?,
            );
            let tx = transaction.0.as_ref().unwrap();
            phase = "loading schema";
            // Schema locks remain held while stored function bodies and metadata
            // are read, compiled and executed. No cross-transaction policy cache.
            let schema = self
                .driver
                .databases()
                .get(&self.database)
                .await?
                .schema()
                .await?;
            let mut rows = GivenRows::new(
                vec![
                    "dog_security_tenant".into(),
                    "dog_security_actor".into(),
                    "dog_security_now".into(),
                ],
                1,
            );
            rows.push_row(vec![
                scope.tenant.clone().into(),
                scope.actor.clone().into(),
                chrono::Utc::now().naive_utc().into(),
            ])?;
            let mut result = if write && requires_checkpoints(&pipeline.stages) {
                phase = "executing checkpoint pipeline";
                self.checkpoint_pipeline(tx, &schema, &pipeline.stages, rows)
                    .await?
            } else {
                phase = "compiling query";
                let compiled = compiler::Compiler::new(tx, &self.options, &schema)?
                    .compile(&pipeline.stages, write)
                    .await?;
                phase = "executing query";
                let answer = tx.query_with_rows(&compiled.source, rows).await?;
                let mut result = crate::transactions::typedb_answer_to_http_ok(
                    answer,
                    if write { "write" } else { "read" },
                    "",
                    &self.options,
                )
                .await?;
                // Every outer row has mandatory post-write booleans. They cannot
                // filter away a mutation and are checked before commit or response.
                compiled.check_result(&mut result)?;
                result
            };
            result["ok"].as_object_mut().unwrap().remove("query");
            if write {
                phase = "committing transaction";
                committing = true;
                transaction.0.take().unwrap().commit().await?;
            } else {
                phase = "closing read transaction";
                tx.close().await?;
                transaction.0.take();
            }
            Ok::<_, anyhow::Error>(result)
        };
        let outcome = tokio::time::timeout(self.options.timeout, operation).await;
        // Dropping a driver transaction only schedules a close. Await confirmed
        // rollback/close on pre-commit failure and cancellation, so rejected
        // requests do not leave locks or server-side work behind.
        if let Some(tx) = transaction.0.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), tx.close()).await;
        }
        match outcome {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(error)) => {
                Err(operation_error(error.context(phase), committing, false).into_anyhow())
            }
            Err(error) => {
                Err(
                    operation_error(anyhow::Error::new(error).context(phase), committing, true)
                        .into_anyhow(),
                )
            }
        }
    }

    /// Materialize each mutation's typed output inside the SAME transaction.
    /// No JSON round-trip, partial commit, or replay of preceding writes.
    async fn checkpoint_pipeline(
        &self,
        tx: &Transaction,
        schema: &str,
        stages: &[typeql::query::stage::Stage],
        mut rows: GivenRows,
    ) -> Result<Value> {
        use std::collections::BTreeSet;
        use typeql::query::stage::Stage;
        ensure!(stages.len() <= 32, "Pipeline stage limit exceeded");
        let (header, initial) = rows.into_parts();
        let mut names = vec![
            "dog_security_tenant".into(),
            "dog_security_actor".into(),
            "dog_security_now".into(),
            "dog_security_lineage".into(),
        ];
        ensure!(header.width() == 3, "Unexpected scope header");
        rows = GivenRows::new(std::mem::take(&mut names), initial.len());
        for mut values in initial {
            values.push(0i64.into());
            rows.push_row(values)?;
        }
        let lineage_input = (
            "dog_security_lineage".to_string(),
            "integer".to_string(),
            false,
        );
        let mut inputs = vec![lineage_input];
        let mut expected = BTreeSet::from([0i64]);
        let mut start = 0;
        let mut wrote = false;
        let metadata = compiler::SchemaMetadata::default();
        for (index, stage) in stages.iter().enumerate() {
            if !is_mutation(stage) {
                continue;
            }
            let mut chunk = stages[start..=index].to_vec();
            let QueryStructure::Pipeline(terminal) =
                typeql::parse_query("match let $seed=1; fetch {'checkpoint': true};")?.structure
            else {
                unreachable!()
            };
            chunk.push(terminal.stages.last().unwrap().clone());
            let compiled = compiler::Compiler::new(tx, &self.options, schema)?
                .with_metadata(metadata.clone())
                .compile_input(&chunk, true, &inputs, true)
                .await
                .with_context(|| format!("Compiling mutation checkpoint {index}"))?;
            let mut stream = tx
                .query_with_rows(&compiled.source, rows)
                .await?
                .into_rows();
            let mut materialized = Vec::new();
            let mut bytes = 0usize;
            let mut observed = BTreeSet::new();
            while let Some(row) = stream.next().await {
                let row = row?;
                let lineage = row
                    .get("dog_security_lineage")?
                    .and_then(|c| c.try_get_integer())
                    .ok_or_else(|| anyhow::anyhow!("Missing checkpoint lineage"))?;
                ensure!(expected.contains(&lineage), "Unexpected checkpoint lineage");
                observed.insert(lineage);
                ensure!(
                    materialized.len() < self.options.max_answers,
                    "Checkpoint row limit exceeded"
                );
                for key in &compiled.checks {
                    ensure!(
                        row.get(&format!("dog_security_check_{key}"))?
                            .and_then(|c| c.try_get_boolean())
                            == Some(true),
                        "Checkpoint policy denied"
                    );
                }
                // Keep attribute concepts as attributes, not just their values.
                // A later query must not silently change TypeQL binding semantics.
                let mut values = Vec::new();
                let mut shape = Vec::new();
                for name in row.get_column_names() {
                    if name.starts_with("dog_security_check_") {
                        continue;
                    }
                    let concept = row
                        .get(name)?
                        .ok_or_else(|| anyhow::anyhow!("Empty checkpoint binding"))?
                        .clone();
                    bytes = bytes
                        .checked_add(format!("{concept:?}").len())
                        .ok_or_else(|| anyhow::anyhow!("Checkpoint size overflow"))?;
                    ensure!(
                        bytes <= self.options.max_response_bytes,
                        "Checkpoint byte limit exceeded"
                    );
                    let object = concept.is_entity() || concept.is_relation();
                    let ty = if concept.is_value() {
                        concept
                            .try_get_value_type()
                            .ok_or_else(|| anyhow::anyhow!("Missing value type"))?
                            .to_string()
                    } else {
                        concept.get_label().to_string()
                    };
                    shape.push((name.clone(), ty, object));
                    values.push(typedb_driver::given::GivenRowEntry::try_from(concept)?);
                }
                if let Some((previous, _)) = materialized.first() {
                    ensure!(
                        previous == &shape,
                        "Heterogeneous checkpoint bindings are unsupported"
                    );
                }
                materialized.push((shape, values));
            }
            if materialized.is_empty() {
                ensure!(
                    !wrote,
                    "Later mutation denied; rolling back preceding writes"
                );
                let result = serde_json::json!({"ok":{
                    "queryType":"write", "answerType":"conceptDocuments",
                    "answers":[], "warning":null
                }});
                ensure!(
                    serde_json::to_vec(&result)?.len() <= self.options.max_response_bytes,
                    "Response byte limit exceeded"
                );
                return Ok(result);
            }
            ensure!(
                !wrote || observed == expected,
                "Later stage discarded mutation rows; rolling back"
            );
            wrote = true;
            let shape = materialized[0].0.clone();
            inputs = shape
                .iter()
                .filter(|(n, _, _)| !n.starts_with("dog_security_") || n == "dog_security_lineage")
                .cloned()
                .collect();
            let lineage_index = shape
                .iter()
                .position(|(n, _, _)| n == "dog_security_lineage")
                .ok_or_else(|| anyhow::anyhow!("Missing lineage column"))?;
            expected = (0..materialized.len()).map(|n| n as i64).collect();
            rows = GivenRows::new(
                shape.into_iter().map(|(n, _, _)| n).collect(),
                materialized.len(),
            );
            for (index, (_, mut values)) in materialized.into_iter().enumerate() {
                values[lineage_index] = (index as i64).into();
                rows.push_row(values)?;
            }
            start = index + 1;
        }
        let tail = &stages[start..];
        ensure!(
            !tail.is_empty() && matches!(tail.last(), Some(Stage::Fetch(_))),
            "Explicit final fetch required"
        );
        let compiled = compiler::Compiler::new(tx, &self.options, schema)?
            .with_metadata(metadata)
            .compile_input(tail, false, &inputs, false)
            .await?;
        let mut result = crate::transactions::typedb_answer_to_http_ok(
            tx.query_with_rows(&compiled.source, rows).await?,
            "write",
            "",
            &self.options,
        )
        .await?;
        let observed = result["ok"]["answers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                let value = r["data"]["lineage"]
                    .as_f64()
                    .ok_or_else(|| anyhow::anyhow!("Missing final lineage"))?;
                ensure!(
                    value.is_finite()
                        && value >= 0.0
                        && value.fract() == 0.0
                        && value < self.options.max_answers as f64,
                    "Invalid final lineage"
                );
                Ok(value as i64)
            })
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(
            observed == expected,
            "Final stream discarded mutation rows; rolling back"
        );
        compiled.check_result(&mut result)?;
        // A final filter must never conceal a preceding side effect.
        ensure!(
            !result["ok"]["answers"].as_array().unwrap().is_empty(),
            "Empty final stream after mutation"
        );
        Ok(result)
    }
}
// Preserve the cause for trusted diagnostics, never in the client message/data.
// In particular, a deadline and a policy failure must not become indistinguishable.
fn operation_error(error: anyhow::Error, committing: bool, deadline: bool) -> dog_core::DogError {
    // Only the driver's structured server code proves this commit was rejected.
    // Never infer rollback from text (including a user-controlled literal), a
    // transport error, or a commit deadline. Those outcomes remain unknown.
    let rejected_conflict = committing
        && !deadline
        && error.chain().any(|cause| {
            cause
                .downcast_ref::<typedb_driver::Error>()
                .is_some_and(|error| {
                    matches!(error, typedb_driver::Error::Server(_)) && error.code() == "STC2"
                })
        });
    let public = if rejected_conflict {
        dog_core::DogError::conflict(
            "Concurrent change prevented commit; refresh before submitting again",
        )
    } else if committing {
        dog_core::DogError::unavailable("TypeDB commit outcome unknown; reconcile before retrying")
    } else if deadline {
        dog_core::DogError::timeout("TypeDB policy operation timed out before commit")
    } else {
        dog_core::DogError::general_error("TypeDB policy operation denied or failed before commit")
    };
    public.with_source(error)
}

#[cfg(test)]
#[path = "../tests/isolation/errors.rs"]
mod error_tests;

fn is_mutation(stage: &typeql::query::stage::Stage) -> bool {
    use typeql::query::stage::Stage;
    matches!(
        stage,
        Stage::Insert(_) | Stage::Put(_) | Stage::Update(_) | Stage::Delete(_)
    )
}
fn requires_checkpoints(stages: &[typeql::query::stage::Stage]) -> bool {
    stages.iter().position(is_mutation).is_some_and(|first| {
        stages[first + 1..]
            .iter()
            .any(|s| !matches!(s, typeql::query::stage::Stage::Fetch(_)))
    })
}
fn named(variable: &Variable) -> Result<String> {
    ensure!(
        matches!(variable, Variable::Named { optional: None, .. }),
        "Optional/anonymous variables are not supported"
    );
    let name = variable
        .name()
        .ok_or_else(|| anyhow::anyhow!("Named variable required"))?;
    ensure!(!name.starts_with("dog_security_"), "Reserved variable");
    Ok(name.into())
}
fn label(reference: &TypeRef) -> Result<String> {
    match reference {
        TypeRef::Label(label) => Ok(label.to_string()),
        _ => anyhow::bail!("Dynamic types are not supported"),
    }
}
fn literal(expression: &Expression) -> Result<String> {
    ensure!(
        matches!(expression, Expression::Value(_)),
        "Only literal values are supported in this position"
    );
    Ok(expression.to_string())
}
fn parser_budget(source: &str) -> Result<()> {
    let mut quote = None;
    let mut escaped = false;
    let mut comment = false;
    let mut depth = 0usize;
    for ch in source.chars() {
        if comment {
            if ch == '\n' {
                comment = false;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }
        match ch {
            '#' => comment = true,
            '\'' | '"' => quote = Some(ch),
            '(' | '[' | '{' => {
                depth += 1;
                ensure!(depth <= 32, "Query nesting limit exceeded");
            }
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_budget_bounds_syntax_not_literals() {
        assert!(parser_budget(&format!("match {}", "{".repeat(33))).is_err());
        assert!(parser_budget(&format!("match $x has title '{}';", "{".repeat(100))).is_ok());
    }
    #[test]
    fn scope_is_bounded_and_redacted() {
        assert!(AuthenticatedScope::new("", "actor").is_err());
        assert!(AuthenticatedScope::new("tenant", "x".repeat(1025)).is_err());
        let scope = AuthenticatedScope::new("secret-tenant", "secret-actor").unwrap();
        assert!(!format!("{scope:?}").contains("secret"));
    }
}
