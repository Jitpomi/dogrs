use std::sync::Arc;
use anyhow::Result;
use serde_json::Value;
use typedb_driver::TypeDBDriver;
use crate::{execute_typedb_query, TransactionType};

/// Trait for types that can provide TypeDB connection details
pub trait TypeDBState {
    fn driver(&self) -> &Arc<TypeDBDriver>;
    fn database(&self) -> &str;
}

/// Generic TypeDB adapter for handling read/write operations
/// This can be used by any service that needs to interact with TypeDB
pub struct TypeDBAdapter {
    driver: Arc<TypeDBDriver>,
    database: String,
}

impl TypeDBAdapter {
    pub fn new<T: TypeDBState>(state: Arc<T>) -> Self {
        Self { 
            driver: state.driver().clone(),
            database: state.database().to_string(),
        }
    }

    /// Execute a write query (insert, delete, update operations)
    pub async fn write(&self, data: Value) -> Result<Value> {
        let query = data.get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' field"))?;

        execute_typedb_query(&self.driver, &self.database, query, TransactionType::Write).await
    }

    /// Execute a read query (match operations)
    pub async fn read(&self, data: Value) -> Result<Value> {
        let query = data.get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' field"))?;

        execute_typedb_query(&self.driver, &self.database, query, TransactionType::Read).await
    }

    /// Execute a schema query (define operations)
    pub async fn schema(&self, data: Value) -> Result<Value> {
        let query = data.get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' field"))?;

        execute_typedb_query(&self.driver, &self.database, query, TransactionType::Schema).await
    }
}
