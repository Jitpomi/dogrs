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

impl TransactionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransactionType::Read => "read",
            TransactionType::Write => "write",
            TransactionType::Schema => "schema",
        }
    }
}

/// Executes a TypeDB query and returns TypeDB Studio compatible response
pub async fn execute_typedb_query(
    driver: &TypeDBDriver,
    database: &str,
    query: &str,
    transaction_type: TransactionType,
) -> Result<Value> {
    // Use a scoped approach to ensure transaction lifetime is properly managed
    match transaction_type {
        TransactionType::Read => execute_read_query(driver, database, query).await,
        TransactionType::Write => execute_write_query(driver, database, query).await,
        TransactionType::Schema => execute_schema_query(driver, database, query).await,
    }
}

async fn execute_read_query(
    driver: &TypeDBDriver,
    database: &str,
    query: &str,
) -> Result<Value> {
    let transaction = driver.transaction(database, typedb_driver::TransactionType::Read).await
        .map_err(|e| anyhow::anyhow!("Failed to create read transaction: {}", e))?;

    let answer = transaction.query(query).await
        .map_err(|e| anyhow::anyhow!("Failed to execute read query: {}", e))?;

    // Process rows immediately while transaction is alive
    let mut answers = Vec::new();
    let mut rows_stream = answer.into_rows();
    
    while let Some(row_result) = rows_stream.next().await {
        let row = row_result.map_err(|e| anyhow::anyhow!("Failed to get concept row: {}", e))?;
        let mut data_map = Map::new();
        
        for column_name in row.get_column_names() {
            if let Ok(Some(concept)) = row.get(column_name) {
                let concept_json = format_concept(&concept)?;
                data_map.insert(column_name.clone(), concept_json);
            }
        }
        
        answers.push(serde_json::json!({
            "data": data_map,
            "involvedBlocks": [0]
        }));
    }

    // Transaction automatically closes when dropped
    Ok(serde_json::json!({
        "ok": {
            "queryType": "read",
            "answerType": "conceptRows",
            "answers": answers,
            "query": query,
            "warning": null
        }
    }))
}

async fn execute_write_query(
    driver: &TypeDBDriver,
    database: &str,
    query: &str,
) -> Result<Value> {
    let transaction = driver.transaction(database, typedb_driver::TransactionType::Write).await
        .map_err(|e| anyhow::anyhow!("Failed to create write transaction: {}", e))?;

    let answer = transaction.query(query).await
        .map_err(|e| anyhow::anyhow!("Failed to execute write query: {}", e))?;

    // Process rows immediately while transaction is alive
    let mut answers = Vec::new();
    let mut rows_stream = answer.into_rows();
    
    while let Some(row_result) = rows_stream.next().await {
        let row = row_result.map_err(|e| anyhow::anyhow!("Failed to get concept row: {}", e))?;
        let mut data_map = Map::new();
        
        for column_name in row.get_column_names() {
            if let Ok(Some(concept)) = row.get(column_name) {
                let concept_json = format_concept(&concept)?;
                data_map.insert(column_name.clone(), concept_json);
            }
        }
        
        answers.push(serde_json::json!({
            "data": data_map,
            "involvedBlocks": [0]
        }));
    }

    // Commit after processing all data
    transaction.commit().await
        .map_err(|e| anyhow::anyhow!("Failed to commit write transaction: {}", e))?;

    Ok(serde_json::json!({
        "ok": {
            "queryType": "write",
            "answerType": "conceptRows",
            "answers": answers,
            "query": query,
            "warning": null
        }
    }))
}

async fn execute_schema_query(
    driver: &TypeDBDriver,
    database: &str,
    query: &str,
) -> Result<Value> {
    let transaction = driver.transaction(database, typedb_driver::TransactionType::Schema).await
        .map_err(|e| anyhow::anyhow!("Failed to create schema transaction: {}", e))?;

    let _answer = transaction.query(query).await
        .map_err(|e| anyhow::anyhow!("Failed to execute schema query: {}", e))?;

    // Schema queries typically don't return data, just commit immediately
    transaction.commit().await
        .map_err(|e| anyhow::anyhow!("Failed to commit schema transaction: {}", e))?;

    // Add a small delay to allow TypeDB driver cleanup to complete
    sleep(Duration::from_millis(100)).await;

    Ok(serde_json::json!({
        "ok": {
            "queryType": "schema",
            "answerType": "ok",
            "answers": [],
            "query": query,
            "warning": null
        }
    }))
}

/// Loads TypeDB schema from a file
pub async fn load_schema_from_file(
    driver: &TypeDBDriver,
    database: &str,
    schema_paths: &[&str],
) -> Result<Value> {
    println!("Loading TypeDB schema from schema.tql file...");
    
    // Try multiple possible paths to find schema.tql
    let mut schema_content = None;
    for path in schema_paths {
        if let Ok(content) = fs::read_to_string(path) {
            schema_content = Some(content);
            println!("Found schema.tql at: {}", path);
            break;
        }
    }
    
    let schema_content = schema_content
        .ok_or_else(|| anyhow::anyhow!("Failed to find schema.tql in any of the expected locations: {:?}", schema_paths))?;

    // Use our generic execute_typedb_query function for schema loading
    let response = execute_typedb_query(driver, database, &schema_content, TransactionType::Schema).await?;

    println!("TypeDB schema loaded successfully from schema.tql!");
    Ok(response)
}


/// Formats a TypeDB Concept into TypeDB Studio compatible JSON
/// Based on the canonical format from TypeDB forum discussion and TypeDB Studio implementation
fn format_concept(concept: &typedb_driver::concept::Concept) -> Result<Value> {
    match concept {
        typedb_driver::concept::Concept::Entity(entity) => {
            Ok(json!({
                "kind": "entity",
                "iid": entity.iid().to_string(),
                "type": {
                    "kind": "entityType",
                    "label": entity.type_().map(|t| t.label()).unwrap_or("unknown")
                }
            }))
        },
        typedb_driver::concept::Concept::Attribute(attr) => {
            let value_str = attr.value.to_string();
            // Remove quotes from string values for cleaner display
            let clean_value = if value_str.starts_with('"') && value_str.ends_with('"') {
                value_str[1..value_str.len()-1].to_string()
            } else {
                value_str
            };
            
            // Determine actual value type from the attribute value
            let value_type = match &attr.value {
                typedb_driver::concept::value::Value::String(_) => "string",
                typedb_driver::concept::value::Value::Integer(_) => "long",
                typedb_driver::concept::value::Value::Double(_) => "double",
                typedb_driver::concept::value::Value::Boolean(_) => "boolean",
                _ => "string", // Default fallback for other types
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
        },
        typedb_driver::concept::Concept::Relation(rel) => {
            Ok(json!({
                "kind": "relation",
                "iid": rel.iid().to_string(),
                "type": {
                    "kind": "relationType",
                    "label": rel.type_().map(|t| t.label()).unwrap_or("unknown")
                }
            }))
        },
        typedb_driver::concept::Concept::EntityType(entity_type) => {
            Ok(json!({
                "kind": "entityType",
                "label": entity_type.label()
            }))
        },
        typedb_driver::concept::Concept::AttributeType(attr_type) => {
            Ok(json!({
                "kind": "attributeType",
                "label": attr_type.label(),
                "valueType": match attr_type.value_type() {
                    Some(vt) => format!("{:?}", vt).to_lowercase(),
                    None => "unknown".to_string()
                }
            }))
        },
        typedb_driver::concept::Concept::RelationType(rel_type) => {
            Ok(json!({
                "kind": "relationType",
                "label": rel_type.label()
            }))
        },
        typedb_driver::concept::Concept::RoleType(role_type) => {
            Ok(json!({
                "kind": "roleType",
                "label": role_type.label()
            }))
        },
        _ => {
            Ok(json!({
                "kind": "other",
                "label": concept.get_label(),
                "value": concept.to_string()
            }))
        }
    }
}
