use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use typedb_driver::TypeDBDriver;

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
    options: crate::QueryOptions,
}

impl TypeDBAdapter {
    pub fn new<T: TypeDBState>(state: Arc<T>) -> Self {
        Self {
            driver: state.driver().clone(),
            database: state.database().to_string(),
            options: crate::QueryOptions::default(),
        }
    }

    pub fn with_options(mut self, options: crate::QueryOptions) -> Result<Self> {
        options.validate()?;
        self.options = options;
        Ok(self)
    }

    /// Execute a write query (insert, delete, update operations)
    pub async fn write(&self, data: Value) -> Result<Value> {
        let query = data
            .get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' field"))?;

        crate::execute_query_with_options(
            &self.driver,
            &self.database,
            query,
            crate::TransactionType::Write,
            &self.options,
        )
        .await
    }

    /// Execute a read query (match operations)
    /// Forces TransactionType::Read - TypeDB will reject DELETE/INSERT operations
    pub async fn read(&self, data: Value) -> Result<Value> {
        let query = data
            .get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' field"))?;

        crate::execute_query_with_options(
            &self.driver,
            &self.database,
            query,
            crate::TransactionType::Read,
            &self.options,
        )
        .await
    }

    /// Execute a schema query (define operations)
    pub async fn schema(&self, data: Value) -> Result<Value> {
        let query = data
            .get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' field"))?;

        crate::execute_query_with_options(
            &self.driver,
            &self.database,
            query,
            crate::TransactionType::Schema,
            &self.options,
        )
        .await
    }
}
