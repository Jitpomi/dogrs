use anyhow::Result;
use futures::StreamExt;
use serde_json::{json, Map, Value};
use tokio::time::Duration;
use typedb_driver::TypeDBDriver;

#[derive(Debug, Clone)]
pub enum TransactionType {
    Read,
    Write,
    Schema,
}

#[derive(Debug, Clone)]
pub enum QueryType {
    Define,
    Undefine,
    Redefine,
    Match,
    Fetch,
    Insert,
    Delete,
    Update,
}

impl QueryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            QueryType::Define => "define",
            QueryType::Undefine => "undefine",
            QueryType::Redefine => "redefine",
            QueryType::Match => "match",
            QueryType::Fetch => "fetch",
            QueryType::Insert => "insert",
            QueryType::Delete => "delete",
            QueryType::Update => "update",
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueryAnalysis {
    pub primary_type: QueryType,
    pub has_aggregation: bool,
    pub has_sorting: bool,
    pub has_pagination: bool,
    pub has_functions: bool,
    pub transaction_type: TransactionType,
    pub returns_document_stream: bool,
}

impl TransactionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransactionType::Read => "read",
            TransactionType::Write => "write",
            TransactionType::Schema => "schema",
        }
    }
}

/// Lexical tokens used only for routing, not authorization or full TypeQL validation.
/// Ignore quoted values, escaped quotes, comments, and variable names. Keep stage
/// delimiters so an identifier in a pattern is not mistaken for a pipeline stage.
fn query_tokens(query: &str) -> Vec<String> {
    let mut chars = query.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(c) = chars.next() {
        match c {
            '#' => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '\'' | '"' => {
                while let Some(next) = chars.next() {
                    if next == '\\' {
                        chars.next();
                    } else if next == c {
                        break;
                    }
                }
                tokens.push("<literal>".into());
            }
            '$' => {
                while chars
                    .peek()
                    .is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                {
                    chars.next();
                }
                tokens.push("<variable>".into());
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut word = String::from(c);
                while chars
                    .peek()
                    .is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                {
                    word.push(chars.next().unwrap());
                }
                tokens.push(word.to_lowercase());
            }
            c if !c.is_whitespace() => tokens.push(c.to_string()),
            _ => {}
        }
    }
    tokens
}

/// Analyze a pipeline for automatic transaction routing. The server still parses
/// and validates TypeQL. Prefer explicit transaction APIs for untrusted queries.
pub fn analyze_query(query: &str) -> QueryAnalysis {
    let tokens = query_tokens(query);
    let first = tokens.first().map(String::as_str).unwrap_or("");
    let primary_type = match first {
        "define" => QueryType::Define,
        "undefine" => QueryType::Undefine,
        "redefine" => QueryType::Redefine,
        "fetch" => QueryType::Fetch,
        "insert" => QueryType::Insert,
        "delete" => QueryType::Delete,
        "update" => QueryType::Update,
        _ => QueryType::Match,
    };
    let mut stages = Vec::new();
    let mut depth = 0usize;
    let mut boundary = true;
    for token in &tokens {
        if depth == 0 && boundary && token.chars().next().is_some_and(char::is_alphabetic) {
            stages.push(token.as_str());
        }
        match token.as_str() {
            "{" | "(" | "[" => {
                depth += 1;
                boundary = false;
            }
            "}" | ")" | "]" => {
                depth = depth.saturating_sub(1);
                boundary = false;
            }
            ";" if depth == 0 => boundary = true,
            _ => boundary = false,
        }
    }
    let has = |word: &str| stages.contains(&word);
    let transaction_type = if matches!(first, "define" | "undefine" | "redefine") {
        TransactionType::Schema
    } else if ["insert", "delete", "update", "put"].iter().any(|s| has(s)) {
        TransactionType::Write
    } else {
        TransactionType::Read
    };
    QueryAnalysis {
        primary_type,
        has_aggregation: has("reduce")
            || tokens.iter().any(|s| {
                matches!(
                    s.as_str(),
                    "count" | "sum" | "max" | "min" | "mean" | "median" | "std"
                )
            }),
        has_sorting: has("sort") || has("order"),
        has_pagination: has("limit") || has("offset"),
        has_functions: tokens.iter().any(|s| matches!(s.as_str(), "fun" | "let")),
        transaction_type,
        returns_document_stream: has("fetch"),
    }
}

/// Limits apply to one complete operation, including commit. A response over a
/// budget fails instead of returning a partial success or committing a write.
#[derive(Clone, Debug)]
pub struct QueryOptions {
    pub max_answers: usize,
    pub max_response_bytes: usize,
    pub max_query_bytes: usize,
    pub timeout: Duration,
}
impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            max_answers: 10_000,
            max_response_bytes: 8 * 1024 * 1024,
            max_query_bytes: 1024 * 1024,
            timeout: Duration::from_secs(30),
        }
    }
}
impl QueryOptions {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.max_answers > 0 && self.max_response_bytes > 0 && self.max_query_bytes > 0,
            "Query limits must be positive"
        );
        anyhow::ensure!(
            !self.timeout.is_zero() && self.timeout <= Duration::from_secs(86400),
            "Query timeout must be positive and at most one day"
        );
        Ok(())
    }
    fn validate_query(&self, query: &str) -> Result<()> {
        self.validate()?;
        anyhow::ensure!(!query.trim().is_empty(), "Query must not be blank");
        anyhow::ensure!(
            query.len() <= self.max_query_bytes,
            "Query byte limit exceeded"
        );
        Ok(())
    }
}

/// Automatic routing is a convenience for trusted queries; never use it as an
/// authorization boundary. Explicit adapter read/write/schema modes cannot escalate.
pub async fn execute_typedb_query(
    driver: &TypeDBDriver,
    database: &str,
    query: &str,
) -> Result<Value> {
    let options = QueryOptions::default();
    options.validate_query(query)?;
    execute_query_with_options(
        driver,
        database,
        query,
        analyze_query(query).transaction_type,
        &options,
    )
    .await
}

pub async fn execute_read_transaction(
    driver: &TypeDBDriver,
    database: &str,
    query: &str,
) -> Result<Value> {
    execute_query_with_options(
        driver,
        database,
        query,
        TransactionType::Read,
        &QueryOptions::default(),
    )
    .await
}
pub async fn execute_write_query(
    driver: &TypeDBDriver,
    database: &str,
    query: &str,
) -> Result<Value> {
    execute_query_with_options(
        driver,
        database,
        query,
        TransactionType::Write,
        &QueryOptions::default(),
    )
    .await
}
pub async fn execute_schema_query(
    driver: &TypeDBDriver,
    database: &str,
    query: &str,
) -> Result<Value> {
    execute_query_with_options(
        driver,
        database,
        query,
        TransactionType::Schema,
        &QueryOptions::default(),
    )
    .await
}

pub async fn execute_query_with_options(
    driver: &TypeDBDriver,
    database: &str,
    query: &str,
    mode: TransactionType,
    options: &QueryOptions,
) -> Result<Value> {
    options.validate_query(query)?;
    let mut committing = false;
    let operation = async {
        let driver_mode = match mode {
            TransactionType::Read => typedb_driver::TransactionType::Read,
            TransactionType::Write => typedb_driver::TransactionType::Write,
            TransactionType::Schema => typedb_driver::TransactionType::Schema,
        };
        let tx = driver
            .transaction_with_options(
                database,
                driver_mode,
                typedb_driver::TransactionOptions::new()
                    .transaction_timeout(options.timeout)
                    .schema_lock_acquire_timeout(options.timeout),
            )
            .await?;
        let answer = tx.query(query).await?;
        let result = typedb_answer_to_http_ok(answer, mode.as_str(), query, options).await?;
        if !matches!(mode, TransactionType::Read) {
            committing = true;
            tx.commit().await.map_err(|e| {
                anyhow::anyhow!(
                    "Commit failed; outcome may be unknown; reconcile before retrying: {e}"
                )
            })?;
        }
        Ok(result)
    };
    match tokio::time::timeout(options.timeout, operation).await {
        Ok(result) => result,
        Err(_) if committing => anyhow::bail!(
            "TypeDB deadline exceeded during commit; outcome unknown; reconcile before retrying"
        ),
        Err(_) => anyhow::bail!("TypeDB deadline exceeded before commit"),
    }
}

fn envelope(query_type: &str, answer_type: &str, query: &str, answers: Vec<Value>) -> Value {
    json!({"ok": {"queryType":query_type,"answerType":answer_type,"answers":answers,"query":query,"warning":null}})
}

// Count encoded JSON bytes without allocating another copy of the response.
fn json_size(value: &Value, limit: usize) -> Result<usize> {
    struct Counter {
        count: usize,
        limit: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit - self.count {
                return Err(std::io::Error::other("Response byte limit exceeded"));
            }
            self.count += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { count: 0, limit };
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.count)
}

async fn collect_answers<S>(
    mut stream: S,
    query_type: &str,
    answer_type: &str,
    query: &str,
    options: &QueryOptions,
) -> Result<Value>
where
    S: futures::Stream<Item = Result<Value>> + Unpin,
{
    let mut used = json_size(
        &envelope(query_type, answer_type, query, vec![]),
        options.max_response_bytes,
    )?;
    let mut answers = Vec::new();
    while let Some(answer) = stream.next().await {
        anyhow::ensure!(
            answers.len() < options.max_answers,
            "Answer count limit exceeded; no partial result returned"
        );
        let answer = answer?;
        let comma = usize::from(!answers.is_empty());
        anyhow::ensure!(
            comma <= options.max_response_bytes - used,
            "Response byte limit exceeded"
        );
        used += comma;
        used += json_size(&answer, options.max_response_bytes - used)?;
        answers.push(answer);
    }
    Ok(envelope(query_type, answer_type, query, answers))
}

async fn typedb_answer_to_http_ok(
    answer: typedb_driver::answer::QueryAnswer,
    query_type: &str,
    query: &str,
    options: &QueryOptions,
) -> Result<Value> {
    match answer {
        typedb_driver::answer::QueryAnswer::Ok(_) => {
            let result = envelope(query_type, "ok", query, vec![]);
            json_size(&result, options.max_response_bytes)?;
            Ok(result)
        }
        typedb_driver::answer::QueryAnswer::ConceptDocumentStream(_, stream) => {
            collect_answers(
                stream.map(|result| {
                    result
                        .map(|document| json!({"data": document.into_json(), "involvedBlocks":[0]}))
                        .map_err(anyhow::Error::from)
                }),
                query_type,
                "conceptDocuments",
                query,
                options,
            )
            .await
        }
        typedb_driver::answer::QueryAnswer::ConceptRowStream(_, stream) => {
            collect_answers(
                stream.map(|result| {
                    let row = result?;
                    let mut data = Map::new();
                    for name in row.get_column_names() {
                        // A missing optional value is legitimate; an access error is not.
                        if let Some(concept) = row.get(name)? {
                            data.insert(name.clone(), format_concept(concept)?);
                        }
                    }
                    Ok(json!({"data":data,"involvedBlocks":[0]}))
                }),
                query_type,
                "conceptRows",
                query,
                options,
            )
            .await
        }
    }
}

/// Formats a TypeDB Concept into a Studio-friendly JSON object.
fn format_concept(concept: &typedb_driver::concept::Concept) -> Result<Value> {
    use typedb_driver::concept::Concept;

    match concept {
        Concept::Entity(entity) => Ok(json!({
            "kind": "entity",
            "iid": entity.iid().to_string(),
            "type": { "kind": "entityType", "label": entity.type_().map(|t| t.label()).unwrap_or("unknown") }
        })),

        Concept::Relation(rel) => Ok(json!({
            "kind": "relation",
            "iid": rel.iid().to_string(),
            "type": { "kind": "relationType", "label": rel.type_().map(|t| t.label()).unwrap_or("unknown") }
        })),

        Concept::Attribute(attr) => {
            // attribute.value is typed; render a stable JSON representation
            let value_str = attr.value.to_string();
            let clean_value = if value_str.starts_with('"') && value_str.ends_with('"') {
                value_str[1..value_str.len() - 1].to_string()
            } else {
                value_str
            };

            let value_type = match &attr.value {
                typedb_driver::concept::value::Value::String(_) => "string",
                typedb_driver::concept::value::Value::Integer(_) => "long",
                typedb_driver::concept::value::Value::Double(_) => "double",
                typedb_driver::concept::value::Value::Boolean(_) => "boolean",
                typedb_driver::concept::value::Value::Datetime(_) => "datetime",
                _ => "string",
            };

            Ok(json!({
                "kind": "attribute",
                "value": clean_value,
                "valueType": value_type,
                "type": {
                    "kind": "attributeType",
                    "label": attr.type_().map(|t| t.label()).unwrap_or("unknown"),
                    "valueType": value_type
                }
            }))
        }

        Concept::EntityType(t) => Ok(json!({ "kind": "entityType", "label": t.label() })),
        Concept::RelationType(t) => Ok(json!({ "kind": "relationType", "label": t.label() })),
        Concept::AttributeType(t) => Ok(json!({
            "kind": "attributeType",
            "label": t.label(),
            "valueType": match t.value_type() {
                Some(vt) => format!("{:?}", vt).to_lowercase(),
                None => "unknown".to_string()
            }
        })),
        Concept::RoleType(t) => Ok(json!({ "kind": "roleType", "label": t.label() })),

        // IMPORTANT: values can appear in reduce/group/etc
        Concept::Value(v) => Ok(json!({
            "kind": "value",
            "value": v.to_string()
        })),
    }
}

/// Explicit files are all required and loaded in caller order. A directory loads
/// schema.tql (required) then functions.tql (optional). No search-path fallback.
pub async fn load_schema_from_file(
    driver: &TypeDBDriver,
    database: &str,
    paths: &[&str],
) -> Result<Value> {
    load_schema_with_options(driver, database, paths, &QueryOptions::default()).await
}

async fn schema_sources(paths: &[&str], options: &QueryOptions) -> Result<Vec<(String, String)>> {
    use tokio::io::AsyncReadExt;
    anyhow::ensure!(
        !paths.is_empty() && paths.len() <= 64,
        "Supply 1 to 64 schema paths"
    );
    let mut files = Vec::new();
    for path in paths {
        let path = std::path::PathBuf::from(path);
        let metadata = tokio::fs::metadata(&path).await?;
        if metadata.is_dir() {
            files.push(path.join("schema.tql"));
            let functions = path.join("functions.tql");
            match tokio::fs::metadata(&functions).await {
                Ok(_) => files.push(functions),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        } else {
            files.push(path);
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut remaining = options.max_query_bytes;
    let mut sources = Vec::new();
    for path in files {
        anyhow::ensure!(
            tokio::fs::metadata(&path).await?.is_file(),
            "Schema path must be a regular file"
        );
        anyhow::ensure!(
            seen.insert(tokio::fs::canonicalize(&path).await?),
            "Duplicate schema file: {}",
            path.display()
        );
        let file = tokio::fs::File::open(&path).await?;
        let mut bytes = Vec::new();
        file.take(remaining.saturating_add(1) as u64)
            .read_to_end(&mut bytes)
            .await?;
        anyhow::ensure!(bytes.len() <= remaining, "Schema input byte limit exceeded");
        remaining -= bytes.len();
        let source = String::from_utf8(bytes)?;
        options.validate_query(&source)?;
        sources.push((path.to_string_lossy().into_owned(), source));
    }
    Ok(sources)
}

pub async fn load_schema_with_options(
    driver: &TypeDBDriver,
    database: &str,
    paths: &[&str],
    options: &QueryOptions,
) -> Result<Value> {
    options.validate()?;
    let mut committing = false;
    let operation = async {
        let sources = schema_sources(paths, options).await?;
        let tx = driver
            .transaction_with_options(
                database,
                typedb_driver::TransactionType::Schema,
                typedb_driver::TransactionOptions::new()
                    .transaction_timeout(options.timeout)
                    .schema_lock_acquire_timeout(options.timeout),
            )
            .await?;
        let names: Vec<_> = sources.iter().map(|(name, _)| name.clone()).collect();
        let mut response = json!({"ok":{"loadedFiles":names,"responses":[]}});
        let mut used = json_size(&response, options.max_response_bytes)?;
        for (_, source) in &sources {
            let answer = tx.query(source).await?;
            let result = typedb_answer_to_http_ok(answer, "schema", source, options).await?;
            let responses = response["ok"]["responses"].as_array_mut().unwrap();
            let comma = usize::from(!responses.is_empty());
            anyhow::ensure!(
                comma <= options.max_response_bytes - used,
                "Response byte limit exceeded"
            );
            used += comma;
            used += json_size(&result, options.max_response_bytes - used)?;
            responses.push(result);
        }
        committing = true;
        tx.commit().await.map_err(|e| {
            anyhow::anyhow!(
                "Schema commit failed; outcome may be unknown; reconcile before retrying: {e}"
            )
        })?;
        Ok(response)
    };
    match tokio::time::timeout(options.timeout, operation).await {
        Ok(result) => result,
        Err(_) if committing => anyhow::bail!(
            "Schema deadline exceeded during commit; outcome unknown; reconcile before retrying"
        ),
        Err(_) => anyhow::bail!("Schema deadline exceeded before commit"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    #[tokio::test]
    async fn row_limit_stops_polling_and_never_returns_partial_success() {
        let mut polls = 0;
        let stream = stream::iter((0..100).map(|n| {
            polls += 1;
            Ok(json!({"data":n}))
        }));
        let options = QueryOptions {
            max_answers: 2,
            ..Default::default()
        };
        assert!(
            collect_answers(stream, "read", "conceptRows", "q", &options)
                .await
                .is_err()
        );
        assert_eq!(polls, 3);
    }
    #[tokio::test]
    async fn byte_budget_counts_escaping_envelope_and_commas_exactly() {
        let answers = vec![json!({"data":"\"é\n"}), json!({"data":"two"})];
        let expected = envelope("read", "conceptRows", "q\n", answers.clone());
        let bytes = serde_json::to_vec(&expected).unwrap().len();
        let options = QueryOptions {
            max_response_bytes: bytes,
            ..Default::default()
        };
        let output = collect_answers(
            stream::iter(answers.clone().into_iter().map(Ok)),
            "read",
            "conceptRows",
            "q\n",
            &options,
        )
        .await
        .unwrap();
        assert_eq!(output, expected);
        let options = QueryOptions {
            max_response_bytes: bytes - 1,
            ..options
        };
        assert!(collect_answers(
            stream::iter(answers.into_iter().map(Ok)),
            "read",
            "conceptRows",
            "q\n",
            &options
        )
        .await
        .is_err());
    }
    #[tokio::test]
    async fn answer_errors_are_propagated_instead_of_returning_partial_rows() {
        let stream = stream::iter(vec![
            Ok(json!({"data":1})),
            Err(anyhow::anyhow!("column unavailable")),
        ]);
        let error = collect_answers(stream, "read", "conceptRows", "q", &QueryOptions::default())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("column unavailable"));
    }
    #[test]
    fn invalid_limits_and_queries_are_rejected() {
        assert!(QueryOptions {
            timeout: Duration::ZERO,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(QueryOptions {
            max_answers: 0,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(QueryOptions {
            max_response_bytes: 0,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(QueryOptions::default().validate_query(" ").is_err());
        assert!(QueryOptions {
            max_query_bytes: 1,
            ..Default::default()
        }
        .validate_query("é")
        .is_err());
    }
    #[tokio::test]
    async fn all_explicit_schema_files_are_required_and_duplicates_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("migration-001.tql");
        let missing = dir.path().join("missing.tql");
        std::fs::write(&file, "define entity first;").unwrap();
        let options = QueryOptions::default();
        assert!(schema_sources(
            &[file.to_str().unwrap(), missing.to_str().unwrap()],
            &options
        )
        .await
        .is_err());
        assert!(
            schema_sources(&[file.to_str().unwrap(), file.to_str().unwrap()], &options)
                .await
                .is_err()
        );
        let loaded = schema_sources(&[file.to_str().unwrap()], &options)
            .await
            .unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].1, "define entity first;");
    }
    #[tokio::test]
    async fn schema_directories_require_schema_and_bound_total_input() {
        let dir = tempfile::tempdir().unwrap();
        let paths = [dir.path().to_str().unwrap()];
        std::fs::write(dir.path().join("functions.tql"), "define entity second;").unwrap();
        assert!(schema_sources(&paths, &QueryOptions::default())
            .await
            .is_err());
        std::fs::write(dir.path().join("schema.tql"), "define entity first;").unwrap();
        let sources = schema_sources(&paths, &QueryOptions::default())
            .await
            .unwrap();
        assert_eq!(sources.len(), 2);
        assert!(sources[0].0.ends_with("schema.tql"));
        assert!(sources[1].0.ends_with("functions.tql"));
        let limit = sources.iter().map(|(_, s)| s.len()).sum::<usize>() - 1;
        assert!(schema_sources(
            &paths,
            &QueryOptions {
                max_query_bytes: limit,
                ..Default::default()
            }
        )
        .await
        .is_err());
        std::fs::write(dir.path().join("schema.tql"), "").unwrap();
        assert!(schema_sources(&paths, &QueryOptions::default())
            .await
            .is_err());
    }
}
