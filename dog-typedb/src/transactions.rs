use anyhow::Result;
use futures::StreamExt;
use serde_json::{json, Map, Value};
use std::fs;
use tokio::time::{sleep, Duration};
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

/// Strict-ish word boundary contains check (prevents "redefine" matching "define" etc.)
fn contains_kw(q: &str, kw: &str) -> bool {
    let q = q.to_lowercase();
    let kw = kw.to_lowercase();
    // crude but effective: require non-alphanumeric boundaries
    q.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .any(|tok| tok == kw)
}

/// fetch { ... } is the reliable signal for concept documents (fetch is terminal)
fn has_fetch_stage(q: &str) -> bool {
    let q = q.to_lowercase();
    // Check for fetch keyword followed by opening brace (with possible whitespace)
    q.contains("fetch") && (q.contains("fetch {") || q.contains("fetch{"))
}

/// Detect write stages anywhere in a pipeline (TypeQL pipelines often start with `match` even for writes)
fn has_write_stage(q: &str) -> bool {
    contains_kw(q, "insert") || contains_kw(q, "delete") || contains_kw(q, "update")
}

fn has_schema_stage(q: &str) -> bool {
    // schema queries start with define/undefine/redefine, but also treat them anywhere as schema
    contains_kw(q, "define") || contains_kw(q, "undefine") || contains_kw(q, "redefine")
}

/// Analyzes a TypeDB query to determine its type and characteristics (pipeline-aware).
pub fn analyze_query(query: &str) -> QueryAnalysis {
    let q = query.trim();
    let q_lower = q.to_lowercase();

    let words: Vec<&str> = q_lower.split_whitespace().collect();
    let first = words.first().copied().unwrap_or("");

    let primary_type = match first {
        "define" => QueryType::Define,
        "undefine" => QueryType::Undefine,
        "redefine" => QueryType::Redefine,
        "match" => QueryType::Match,
        "fetch" => QueryType::Fetch,
        "insert" => QueryType::Insert,
        "delete" => QueryType::Delete,
        "update" => QueryType::Update,
        _ => QueryType::Match,
    };

    let returns_document_stream = has_fetch_stage(&q_lower);
    
    // Debug logging to understand what's happening
    println!("DEBUG: Query analysis for: {}", query);
    println!("DEBUG: Lowercase query: {}", q_lower);
    println!("DEBUG: has_fetch_stage result: {}", returns_document_stream);
    println!("DEBUG: Contains 'fetch': {}", q_lower.contains("fetch"));
    println!("DEBUG: Contains 'fetch {{': {}", q_lower.contains("fetch {"));
    println!("DEBUG: Contains 'fetch{{': {}", q_lower.contains("fetch{"));

    let has_sorting = contains_kw(&q_lower, "sort") || contains_kw(&q_lower, "order");
    let has_pagination = contains_kw(&q_lower, "limit") || contains_kw(&q_lower, "offset");

    // In TypeQL, aggregations commonly come through `reduce`, `group`, and aggregation funcs in expressions
    let has_aggregation =
        contains_kw(&q_lower, "reduce") ||
        contains_kw(&q_lower, "group") ||
        q_lower.contains("count(") ||
        q_lower.contains("sum(") ||
        q_lower.contains("max(") ||
        q_lower.contains("min(") ||
        q_lower.contains("mean(") ||
        q_lower.contains("median(") ||
        q_lower.contains("std(");

    // conservative flag; TypeQL function defs vary by version
    let has_functions = q_lower.contains("function") || q_lower.contains("let ");

    let transaction_type = if has_schema_stage(&q_lower) && matches!(first, "define" | "undefine" | "redefine") {
        TransactionType::Schema
    } else if has_schema_stage(&q_lower) && !has_write_stage(&q_lower) && !contains_kw(&q_lower, "match") {
        // fallback: schema-ish blobs
        TransactionType::Schema
    } else if has_write_stage(&q_lower) {
        TransactionType::Write
    } else {
        TransactionType::Read
    };

    QueryAnalysis {
        primary_type,
        has_aggregation,
        has_sorting,
        has_pagination,
        has_functions,
        transaction_type,
        returns_document_stream,
    }
}

/// Executes a TypeDB query and returns TypeDB Studio/HTTP compatible response.
/// Response schema: answerType is ONLY ok|conceptRows|conceptDocuments.
pub async fn execute_typedb_query(
    driver: &TypeDBDriver,
    database: &str,
    query: &str,
) -> Result<Value> {
    println!("DEBUG: execute_typedb_query called with query: {}", query);
    
    let analysis = analyze_query(query);
    
    println!("DEBUG: Query analysis completed. Transaction type: {:?}", analysis.transaction_type);

    match analysis.transaction_type {
        TransactionType::Read => {
            println!("DEBUG: Dispatching to execute_read_query");
            execute_read_query(driver, database, query).await
        },
        TransactionType::Write => {
            println!("DEBUG: Dispatching to execute_write_query");
            execute_write_query(driver, database, query).await
        },
        TransactionType::Schema => {
            println!("DEBUG: Dispatching to execute_schema_query");
            execute_schema_query(driver, database, query).await
        },
    }
}

async fn execute_read_query(driver: &TypeDBDriver, database: &str, query: &str) -> Result<Value> {
    println!("=== EXECUTE_READ_QUERY START ===");
    println!("Query: {}", query);
    
    let tx = driver
        .transaction(database, typedb_driver::TransactionType::Read)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create read transaction: {}", e))?;

    println!("=== TRANSACTION CREATED ===");

    let answer = tx
        .query(query)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to execute read query: {}", e))?;

    println!("=== TX.QUERY() COMPLETED - ABOUT TO CALL typedb_answer_to_http_ok ===");

    // Consume answer while tx is alive
    let res = typedb_answer_to_http_ok(answer, "read", query, 10_000).await;

    println!("=== typedb_answer_to_http_ok RETURNED ===");

    // read tx auto-closes on drop
    res
}


async fn execute_write_query(driver: &TypeDBDriver, database: &str, query: &str) -> Result<Value> {
    let tx = driver
        .transaction(database, typedb_driver::TransactionType::Write)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create write transaction: {}", e))?;

    let answer = tx
        .query(query)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to execute write query: {}", e))?;

    // IMPORTANT: consume streams before commit (keep tx alive)
    let res = typedb_answer_to_http_ok(answer, "write", query, 10_000).await?;

    tx.commit()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to commit write transaction: {}", e))?;

    Ok(res)
}

async fn execute_schema_query(driver: &TypeDBDriver, database: &str, query: &str) -> Result<Value> {
    let tx = driver
        .transaction(database, typedb_driver::TransactionType::Schema)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create schema transaction: {}", e))?;

    let answer = tx
        .query(query)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to execute schema query: {}", e))?;

    // Schema queries are typically Ok; still handle safely
    let res = typedb_answer_to_http_ok(answer, "schema", query, 10_000).await?;

    tx.commit()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to commit schema transaction: {}", e))?;

    // allow driver cleanup
    sleep(Duration::from_millis(50)).await;

    Ok(res)
}

/// Convert Rust driver QueryAnswer into TypeDB HTTP API-style { ok: { ... } }.
/// Uses official TypeDB driver API: into_documents() and into_rows() with proper type checking.
async fn typedb_answer_to_http_ok(
    answer: typedb_driver::answer::QueryAnswer,
    query_type: &str,
    query: &str,
    max_answers: usize,
) -> Result<Value> {
    // CRITICAL DEBUG: This should appear if our function is called
    println!("=== TYPEDB_ANSWER_TO_HTTP_OK CALLED ===");
    println!("Query: {}", query);
    println!("QueryType: {}", query_type);

    // Use direct enum matching - this is the ONLY safe way to handle QueryAnswer
    match answer {
        typedb_driver::answer::QueryAnswer::Ok(_) => {
            println!("DEBUG: Processing Ok response");
            return Ok(json!({
                "ok": {
                    "queryType": query_type,
                    "answerType": "ok",
                    "answers": [],
                    "query": query,
                    "warning": null
                }
            }));
        }
        typedb_driver::answer::QueryAnswer::ConceptDocumentStream(_, mut stream) => {
            println!("DEBUG: Processing ConceptDocumentStream");
            let mut answers = Vec::new();
            let mut truncated = false;

            while let Some(document_result) = stream.next().await {
                let document = document_result.map_err(|e| anyhow::anyhow!("Failed to get concept document: {}", e))?;
                answers.push(json!({"data": document.into_json(), "involvedBlocks": [0]}));

                if answers.len() >= max_answers {
                    truncated = true;
                    break;
                }
            }

            return Ok(json!({
                "ok": {
                    "queryType": query_type,
                    "answerType": "conceptDocuments",
                    "answers": answers,
                    "query": query,
                    "warning": if truncated { Some(format!("Answer limit reached ({}). Truncated.", max_answers)) } else { None }
                }
            }));
        }

        typedb_driver::answer::QueryAnswer::ConceptRowStream(_, mut stream) => {
            println!("DEBUG: Processing ConceptRowStream");
            let mut answers = Vec::new();
            let mut truncated = false;

            while let Some(row_result) = stream.next().await {
                let row = row_result.map_err(|e| anyhow::anyhow!("Failed to get concept row: {}", e))?;
                let mut data_map = Map::new();

                for column_name in row.get_column_names() {
                    if let Ok(Some(concept)) = row.get(&column_name) {
                        data_map.insert(column_name.clone(), format_concept(&concept)?);
                    }
                }

                answers.push(json!({"data": data_map, "involvedBlocks": [0]}));

                if answers.len() >= max_answers {
                    truncated = true;
                    break;
                }
            }

            return Ok(json!({
                "ok": {
                    "queryType": query_type,
                    "answerType": "conceptRows",
                    "answers": answers,
                    "query": query,
                    "warning": if truncated { Some(format!("Answer limit reached ({}). Truncated.", max_answers)) } else { None }
                }
            }));
        }

        typedb_driver::answer::QueryAnswer::Ok(_) => {
            println!("DEBUG: Processing Ok response");
            return Ok(json!({
                "ok": {
                    "queryType": query_type,
                    "answerType": "ok",
                    "answers": [],
                    "query": query,
                    "warning": null
                }
            }));
        }
    }

    // Fallback for unexpected answer types
    Err(anyhow::anyhow!("Unexpected QueryAnswer type for query: {}", query))
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

        _ => Ok(json!({
            "kind": "other",
            "label": concept.get_label(),
            "value": concept.to_string()
        })),
    }
}

/// Loads TypeDB schema from a file
pub async fn load_schema_from_file(
    driver: &TypeDBDriver,
    database: &str,
    schema_paths: &[&str],
) -> Result<Value> {
    println!("Loading TypeDB schema from schema.tql file...");
    
    let mut schema_content = None;
    for path in schema_paths {
        println!("Checking schema path: {}", path);
        if let Ok(content) = fs::read_to_string(path) {
            println!("Found schema.tql at: {}", path);
            schema_content = Some(content);
            break;
        }
    }

    let schema_content = schema_content.ok_or_else(|| {
        anyhow::anyhow!(
            "Failed to find schema.tql in any expected locations: {:?}",
            schema_paths
        )
    })?;

    let response = execute_typedb_query(driver, database, &schema_content).await?;

    println!("TypeDB schema loaded successfully from schema.tql!");
    Ok(response)
}
