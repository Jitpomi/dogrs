use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use typedb_driver::TypeDBDriver;

/// Trait for types that can provide TypeDB connection details
pub trait TypeDBState {
    fn driver(&self) -> &Arc<TypeDBDriver>;
    fn database(&self) -> &str;
}

/// Trusted, unscoped TypeDB executor. NOT a public tenant authorization boundary.
/// This can be used by any service that needs to interact with TypeDB
pub struct TypeDBAdapter {
    driver: Arc<TypeDBDriver>,
    database: String,
    options: crate::QueryOptions,
}

impl TypeDBAdapter {
    /// Consume the unscoped executor to obtain a fail-closed tenant adapter.
    /// Existing services retain read/write custom methods; pass the request's
    /// authenticated context produced by their before hook.
    pub fn tenant_scoped(self, policy: crate::TenantQueryPolicy) -> TenantTypeDBAdapter {
        TenantTypeDBAdapter {
            driver: self.driver,
            database: self.database,
            options: self.options,
            policy,
        }
    }
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

/// Public-query executor with mandatory tenant query-contract enforcement.
/// There is no unscoped read/write overload, schema method, or raw-adapter escape.
/// Trusted application authentication hooks issue the request-local context.
///
/// ```compile_fail
/// # async fn example(adapter: dog_typedb::TenantTypeDBAdapter) {
/// // A query body cannot substitute for authenticated request context.
/// adapter.read(serde_json::json!({"query":"match $r isa resource;"})).await;
/// # }
/// ```
pub struct TenantTypeDBAdapter {
    driver: Arc<TypeDBDriver>,
    database: String,
    options: crate::QueryOptions,
    policy: crate::TenantQueryPolicy,
}
impl TenantTypeDBAdapter {
    pub async fn read(&self, context: &crate::TenantQueryContext, data: Value) -> Result<Value> {
        self.execute(context, data, crate::QueryMethod::Read).await
    }
    pub async fn write(&self, context: &crate::TenantQueryContext, data: Value) -> Result<Value> {
        self.execute(context, data, crate::QueryMethod::Write).await
    }
    async fn execute(
        &self,
        context: &crate::TenantQueryContext,
        data: Value,
        method: crate::QueryMethod,
    ) -> Result<Value> {
        let prepared = self.policy.prepare(context, data, method)?;
        let mode = match method {
            crate::QueryMethod::Read => crate::TransactionType::Read,
            crate::QueryMethod::Write => crate::TransactionType::Write,
        };
        crate::transactions::execute_query_with_result_check(
            &self.driver,&self.database,&prepared.query,mode,&self.options,
            |result|prepared.check_result(result),
        ).await.map_err(|error| {
            // TypeDB errors can embed the expanded query, including trusted
            // claims. Do not expose or attach that source to public responses.
            if error.downcast_ref::<dog_core::DogError>().is_some() { return error; }
            anyhow::anyhow!(
                "TypeDB operation failed. If commit was attempted, outcome may be unknown; reconcile before retrying."
            )
        })
    }
}
